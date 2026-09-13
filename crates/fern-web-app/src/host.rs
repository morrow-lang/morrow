//! Thread-confined, rooted native ABI boundary; no raw value leaves this module.
use fern_runtime::{
    abi,
    managed::{self, Exec},
    memory,
};
use fern_web_protocol::Error;
use std::ffi::c_void;

unsafe extern "C" {
    fn fern_library_open(fault: *mut i64) -> *mut Exec;
    fn fern_library_string_port(exec: *mut Exec) -> *mut c_void;
    fn fern_export_start_room(fault: *mut i64, exec: *mut Exec) -> *mut c_void;
    fn fern_export_restore_room(
        fault: *mut i64,
        exec: *mut Exec,
        input: *const std::ffi::c_char,
    ) -> *mut abi::ResultValue;
    fn fern_export_send_command(
        fault: *mut i64,
        exec: *mut Exec,
        owner: *mut c_void,
        input: *const std::ffi::c_char,
        reply: *mut c_void,
    ) -> i64;
    fn fern_export_inspect_room(
        fault: *mut i64,
        exec: *mut Exec,
        owner: *mut c_void,
        reply: *mut c_void,
    ) -> i64;
}

struct Rooted {
    // Field order unregisters the range before its stable backing allocation drops.
    _root: memory::Root,
    slot: Box<usize>,
}
impl Rooted {
    fn new(value: usize) -> Self {
        let slot = Box::new(value);
        // SAFETY: Box keeps the word stable; Root is thread confined and drops first.
        let root = unsafe { memory::root_range(&*slot, 1) };
        Self { _root: root, slot }
    }
    fn pointer(&self) -> *mut c_void {
        *self.slot as *mut c_void
    }
}

pub(super) struct Room {
    fault: Box<i64>,
    exec: *mut Exec,
    owner: Rooted,
    port: Rooted,
    clock: Clock,
}

/// Empty in ordinary builds; simulation time stays on the native owner thread.
#[derive(Clone, Default)]
pub(super) struct Clock {
    #[cfg(feature = "simulation")]
    source: Option<std::rc::Rc<std::cell::Cell<u64>>>,
}
impl Clock {
    #[cfg(feature = "simulation")]
    pub(super) fn simulated(source: std::rc::Rc<std::cell::Cell<u64>>) -> Self {
        Self {
            source: Some(source),
        }
    }
    // SAFETY: caller owns a newly opened, rooted session on this thread.
    unsafe fn enable(&self, _exec: *mut Exec) -> Result<(), Error> {
        #[cfg(feature = "simulation")]
        if let Some(source) = &self.source {
            unsafe { managed::simulation::enable_clock(_exec, source.get()) }
                .map_err(|_| Error::ResyncRequired)?;
        }
        Ok(())
    }
    // SAFETY: caller owns a live, rooted session and is outside its callbacks.
    unsafe fn advance(&self, _exec: *mut Exec) -> Result<(), Error> {
        #[cfg(feature = "simulation")]
        if let Some(source) = &self.source {
            unsafe { managed::simulation::advance_clock(_exec, source.get()) }
                .map_err(|_| Error::ResyncRequired)?;
        }
        Ok(())
    }
}
impl Room {
    pub(super) fn new(initial: Option<&str>, clock: Clock) -> Result<Self, Error> {
        let mut room = Self {
            fault: Box::new(0),
            exec: std::ptr::null_mut(),
            owner: Rooted::new(0),
            port: Rooted::new(0),
            clock,
        };
        // SAFETY: generated static descriptors live for the process; fault and roots
        // remain stable until close. All calls execute on this same owning thread.
        unsafe {
            room.exec = fern_library_open(&mut *room.fault);
            if room.exec.is_null() || *room.fault != 0 {
                return Err(Error::ResyncRequired);
            }
            room.clock.enable(room.exec)?;
            *room.port.slot = fern_library_string_port(room.exec) as usize;
            if room.port.pointer().is_null() || *room.fault != 0 {
                return Err(Error::ResyncRequired);
            }
            if let Some(initial) = initial {
                let input = Rooted::new(abi::string(initial) as usize);
                let result =
                    fern_export_restore_room(&mut *room.fault, room.exec, input.pointer().cast());
                if *room.fault != 0 || result.is_null() || (*result).tag != 0 {
                    return Err(Error::ResyncRequired);
                }
                // No allocation occurs between return and rooting the Result's PID.
                *room.owner.slot = (*result).value as usize;
            } else {
                *room.owner.slot = fern_export_start_room(&mut *room.fault, room.exec) as usize;
            }
        }
        if room.owner.pointer().is_null() || *room.fault != 0 {
            return Err(Error::ResyncRequired);
        }
        Ok(room)
    }
    pub(super) fn inspect(&mut self) -> Result<Vec<u8>, Error> {
        // SAFETY: all pointers are live, rooted and belong to this session/thread.
        let status = unsafe {
            self.clock.advance(self.exec)?;
            fern_export_inspect_room(
                &mut *self.fault,
                self.exec,
                self.owner.pointer(),
                self.port.pointer(),
            )
        };
        self.reply(status)
    }
    pub(super) fn command(&mut self, text: &str) -> Result<Vec<u8>, Error> {
        if text.len() > 4096 || text.contains('\0') {
            return Err(Error::Malformed);
        }
        // SAFETY: reject invalid time before allocating or enqueueing application input.
        unsafe {
            self.clock.advance(self.exec)?;
        }
        let input = Rooted::new(abi::string(text) as usize);
        // SAFETY: input is rooted UTF8 with a terminator; generated function never
        // retains a Rust borrow and send copies its typed payload into the room heap.
        let status = unsafe {
            fern_export_send_command(
                &mut *self.fault,
                self.exec,
                self.owner.pointer(),
                input.pointer().cast(),
                self.port.pointer(),
            )
        };
        self.reply(status)
    }
    fn reply(&mut self, status: i64) -> Result<Vec<u8>, Error> {
        if status != 0 || *self.fault != 0 {
            return Err(Error::Malformed);
        }
        // SAFETY: bounded calls borrow the rooted open session and String port;
        // bytes are copied into Rust storage before any managed root is released.
        unsafe {
            self.clock.advance(self.exec)?;
            let status = managed::fern_managed_poll(self.exec, 4096);
            if status != 1 || *self.fault != 0 {
                return Err(Error::Malformed);
            }
            let length = managed::fern_managed_port_peek_len(self.exec, self.port.pointer());
            if !(0..=65536).contains(&length) {
                return Err(Error::Malformed);
            }
            let mut bytes = vec![0; length as usize];
            let copied = managed::fern_managed_port_read(
                self.exec,
                self.port.pointer(),
                bytes.as_mut_ptr(),
                bytes.len(),
            );
            if copied != length
                || managed::fern_managed_port_peek_len(self.exec, self.port.pointer()) != -1
            {
                return Err(Error::Malformed);
            }
            std::str::from_utf8(&bytes).map_err(|_| Error::Malformed)?;
            Ok(bytes)
        }
    }
}
impl Drop for Room {
    fn drop(&mut self) {
        if !self.exec.is_null() {
            // SAFETY: close runs once while its fault cell, PID roots and thread live.
            unsafe { managed::fern_managed_close(self.exec) };
        }
    }
}
