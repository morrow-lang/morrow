//! Standalone retained-child capture with bounded protocol publication.
mod spool;
mod sys;
use std::{
    ffi::{CString, OsString},
    os::{fd::OwnedFd, unix::ffi::OsStrExt},
    time::{Duration, Instant},
};
use sys::{close, errno, fd};
const LIMIT: usize = 256 * 1024;
const BUDGET: usize = 65536;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Error {
    Invalid = 1,
    Spawn = 2,
    Time = 3,
    Cap = 4,
    Io = 5,
    Signal = 6,
    Reaper = 7,
    Parent = 8,
}
#[derive(Default)]
struct Stream {
    reader: Option<OwnedFd>,
    writer: Option<OwnedFd>,
    file: Option<OwnedFd>,
    length: usize,
    bytes: Vec<u8>,
}
struct State {
    streams: [Stream; 2],
    input: Option<OwnedFd>,
    parent: Option<OwnedFd>,
    directory: Option<OwnedFd>,
    identity: Option<libc::stat>,
    created: bool,
    child: libc::pid_t,
    retained: bool,
    finished: bool,
    status: i32,
    error: Option<Error>,
    deadline: Instant,
    writes: usize,
}
impl State {
    fn new() -> Self {
        Self {
            streams: Default::default(),
            input: None,
            parent: None,
            directory: None,
            identity: None,
            created: false,
            child: 0,
            retained: false,
            finished: false,
            status: 0,
            error: None,
            deadline: Instant::now(),
            writes: 0,
        }
    }
    fn fail(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    fn remaining(&mut self) -> Duration {
        if sys::interrupted() {
            self.fail(Error::Signal);
        }
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            self.fail(Error::Time);
        }
        left
    }
    fn parent_alive(&mut self) {
        let mut poll = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let result = sys::poll(std::slice::from_mut(&mut poll), 0);
        if result < 0 && errno() != libc::EINTR {
            self.fail(Error::Io);
        } else if result > 0 {
            if poll.revents & (libc::POLLHUP | libc::POLLIN) != 0 {
                self.fail(Error::Parent);
            } else if poll.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                self.fail(Error::Io);
            }
        }
    }
    fn descriptors(&mut self) -> bool {
        self.input = sys::open(c"/dev/null", libc::O_RDONLY | libc::O_CLOEXEC, 0);
        if self.input.is_none() {
            return false;
        }
        for stream in &mut self.streams {
            let Some((reader, writer)) = sys::pipe() else {
                return false;
            };
            stream.reader = Some(reader);
            stream.writer = Some(writer);
        }
        true
    }
    fn spawn(&mut self, args: &[CString]) {
        if self.remaining().is_zero() || self.error.is_some() {
            return;
        }
        match sys::spawn(
            args,
            fd(&self.input),
            [
                fd(&self.streams[0].reader),
                fd(&self.streams[0].writer),
                fd(&self.streams[1].reader),
                fd(&self.streams[1].writer),
            ],
        ) {
            Ok(pid) => {
                self.child = pid;
                self.retained = true;
            }
            Err(error) => self.fail(error),
        }
    }
    fn observe(&mut self) {
        match sys::observe(self.child) {
            Ok(done) => self.finished = done,
            Err(code) => {
                if code == libc::ECHILD {
                    self.retained = false;
                }
                if code != libc::EINTR {
                    self.fail(Error::Reaper);
                }
            }
        }
    }
    fn read_stream(&mut self, index: usize) -> bool {
        let source = fd(&self.streams[index].reader);
        if source < 0 {
            return false;
        }
        let room = LIMIT - self.streams[index].length;
        let mut bytes = [0u8; 4096];
        let request = bytes.len().min(room + 1);
        let count = sys::read(source, &mut bytes[..request]);
        let code = errno();
        if count > 0 {
            let count = count as usize;
            if count > room {
                self.fail(Error::Cap);
            }
            let accepted = count.min(room);
            let mut offset = 0;
            while offset < accepted {
                self.writes += 1;
                if self.writes > BUDGET {
                    self.fail(Error::Io);
                    break;
                }
                let n = sys::write(fd(&self.streams[index].file), &bytes[offset..accepted]);
                if n > 0 {
                    offset += n as usize;
                } else if n < 0 && errno() == libc::EINTR {
                    self.remaining();
                    if self.error.is_some() {
                        break;
                    }
                } else {
                    self.fail(Error::Io);
                    break;
                }
            }
            self.streams[index].length += accepted;
        } else if count == 0 {
            if !close(&mut self.streams[index].reader) {
                self.fail(Error::Io);
            }
        } else if code != libc::EINTR && code != libc::EAGAIN && code != libc::EWOULDBLOCK {
            self.fail(Error::Io);
        }
        self.remaining();
        count > 0 || (count < 0 && code == libc::EINTR)
    }
    fn poll_streams(&mut self, milliseconds: i32) {
        let mut polls = [
            libc::pollfd {
                fd: fd(&self.streams[0].reader),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: fd(&self.streams[1].reader),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if sys::poll(&mut polls, milliseconds) < 0 && errno() != libc::EINTR {
            self.fail(Error::Io);
        }
        if polls.iter().any(|p| p.revents & libc::POLLNVAL != 0) {
            self.fail(Error::Io);
        }
    }
    fn capture(&mut self) {
        for _ in 0..BUDGET {
            if self.error.is_some() || self.finished {
                break;
            }
            self.parent_alive();
            let left = self.remaining();
            if self.error.is_some() {
                break;
            }
            self.observe();
            if self.error.is_some() || self.finished {
                break;
            }
            self.read_stream(0);
            if self.error.is_none() {
                self.read_stream(1);
            }
            if self.error.is_none() {
                self.poll_streams(left.as_millis().min(10) as i32);
            }
        }
        if !self.finished && self.error.is_none() {
            self.fail(Error::Io);
        }
    }
    fn cleanup_child(&mut self) {
        if !self.retained {
            return;
        }
        if !sys::kill_group(self.child, self.finished) {
            self.fail(Error::Io);
        }
        match sys::reap(self.child) {
            Some(status) => self.status = status,
            None => self.fail(Error::Io),
        }
        self.retained = false;
    }
    fn finish_streams(&mut self) {
        for _ in 0..BUDGET {
            if self.error.is_some() || self.streams.iter().all(|s| s.reader.is_none()) {
                break;
            }
            self.parent_alive();
            let first = self.read_stream(0);
            let second = if self.error.is_none() {
                self.read_stream(1)
            } else {
                false
            };
            if !first && !second && self.error.is_none() {
                self.poll_streams(1);
            }
        }
        if self.streams.iter().any(|s| s.reader.is_some()) {
            self.fail(Error::Io);
        }
        if !close(&mut self.input) {
            self.fail(Error::Io);
        }
        for index in 0..2 {
            if !close(&mut self.streams[index].reader) {
                self.fail(Error::Io);
            }
            if !close(&mut self.streams[index].writer) {
                self.fail(Error::Io);
            }
        }
    }
    fn execute(&mut self, path: &CString, args: &[CString]) {
        self.parent_alive();
        if self.error.is_none() && !self.spool_files(path) {
            self.fail(Error::Io);
        }
        if self.error.is_none() && !self.descriptors() {
            self.fail(Error::Io);
        }
        if self.error.is_none() {
            self.spawn(args);
        }
        if !close(&mut self.input) {
            self.fail(Error::Io);
        }
        for index in 0..2 {
            if !close(&mut self.streams[index].writer) {
                self.fail(Error::Io);
            }
        }
        if self.retained && self.error.is_none() {
            self.capture();
        }
        self.cleanup_child();
        self.finish_streams();
        for index in 0..2 {
            if self.streams[index].file.is_some() {
                self.load_spool(index);
            }
        }
        self.remove_spools();
    }
}
fn arguments(args: &[OsString], state: &mut State) -> Option<(CString, Vec<CString>)> {
    if args.len() < 5 || args.len() > 4100 || args[3] != "--" {
        return None;
    }
    let timeout = args[1].as_bytes();
    if timeout.is_empty() || timeout.len() >= 20 || !timeout.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let timeout = std::str::from_utf8(timeout).ok()?.parse::<u64>().ok()?;
    if !(1..=60000).contains(&timeout) {
        return None;
    }
    if !args[2].as_bytes().starts_with(b"/")
        || args[2].as_bytes().len() >= 4096
        || !args[4].as_bytes().starts_with(b"/")
    {
        return None;
    }
    let mut room = 1024 * 1024usize;
    let mut command = Vec::new();
    for arg in &args[4..] {
        room = room.checked_sub(arg.as_bytes().len() + 1)?;
        command.push(CString::new(arg.as_bytes()).ok()?);
    }
    state.deadline = Instant::now().checked_add(Duration::from_millis(timeout))?;
    Some((CString::new(args[2].as_bytes()).ok()?, command))
}
fn main() -> std::process::ExitCode {
    if !sys::protocol_channels() {
        return std::process::ExitCode::from(125);
    }
    let mut state = State::new();
    let args: Vec<_> = std::env::args_os().collect();
    match arguments(&args, &mut state) {
        None => state.fail(Error::Invalid),
        Some((path, command)) => {
            if !sys::signals() {
                state.fail(Error::Reaper);
            } else {
                state.execute(&path, &command);
            }
        }
    }
    std::process::ExitCode::from(state.publish())
}
