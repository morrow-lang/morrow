//! Exclusive private spools and byte-exact, bounded protocol publication.
use super::{BUDGET, Error, LIMIT, State, close, fd, sys};
use std::{
    ffi::CStr,
    time::{Duration, Instant},
};
fn private_directory(fd: i32) -> bool {
    sys::stat(fd).is_some_and(|s| {
        s.st_mode & libc::S_IFMT == libc::S_IFDIR
            && s.st_uid == sys::uid()
            && s.st_mode & 0o777 == 0o700
    })
}
fn identity(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev && a.st_ino == b.st_ino
}
impl State {
    pub(super) fn spool_files(&mut self, path: &CStr) -> bool {
        let flags = libc::O_RDONLY
            | libc::O_DIRECTORY
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC;
        self.parent = sys::open(path, flags, 0);
        if !private_directory(fd(&self.parent)) || !sys::mkdir_at(fd(&self.parent), c"capture") {
            return false;
        }
        self.created = true;
        self.directory = sys::open_at(fd(&self.parent), c"capture", flags, 0);
        if !private_directory(fd(&self.directory)) {
            return false;
        }
        self.identity = sys::stat(fd(&self.directory));
        for (index, name) in [c"stdout", c"stderr"].into_iter().enumerate() {
            self.streams[index].file = sys::open_at(
                fd(&self.directory),
                name,
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC,
                0o600,
            );
            if self.streams[index].file.is_none() {
                return false;
            }
        }
        true
    }
    pub(super) fn load_spool(&mut self, index: usize) {
        let stream = &mut self.streams[index];
        let descriptor = fd(&stream.file);
        let valid = sys::stat(descriptor).is_some_and(|s| {
            s.st_mode & libc::S_IFMT == libc::S_IFREG
                && s.st_uid == sys::uid()
                && s.st_mode & 0o777 == 0o600
                && s.st_nlink == 1
                && s.st_size >= 0
                && s.st_size as u64 == stream.length as u64
        });
        if !valid {
            stream.length = 0;
            self.fail(Error::Io);
            return;
        }
        stream.bytes.resize(stream.length, 0);
        let mut offset = 0;
        for _ in 0..BUDGET {
            if offset == stream.length {
                break;
            }
            let count = sys::pread(descriptor, &mut stream.bytes[offset..], offset);
            if count > 0 {
                offset += count as usize;
            } else if count < 0 && sys::errno() == libc::EINTR {
                continue;
            } else {
                break;
            }
        }
        if offset != stream.length {
            stream.bytes.clear();
            stream.length = 0;
            self.fail(Error::Io);
        }
    }
    pub(super) fn remove_spools(&mut self) {
        for (index, name) in [c"stdout", c"stderr"].into_iter().enumerate() {
            if self.streams[index].file.is_none() {
                continue;
            }
            let owned = sys::stat(fd(&self.streams[index].file));
            let named = sys::stat_at(fd(&self.directory), name);
            if !owned.zip(named).is_some_and(|(a, b)| identity(&a, &b))
                || !sys::unlink_at(fd(&self.directory), name, false)
            {
                self.fail(Error::Io);
            }
            if !close(&mut self.streams[index].file) {
                self.fail(Error::Io);
            }
        }
        if self.created {
            let named = sys::stat_at(fd(&self.parent), c"capture");
            if !self
                .identity
                .as_ref()
                .zip(named.as_ref())
                .is_some_and(|(a, b)| identity(a, b))
                || !sys::unlink_at(fd(&self.parent), c"capture", true)
            {
                self.fail(Error::Io);
            }
        }
        if !close(&mut self.directory) {
            self.fail(Error::Io);
        }
        if !close(&mut self.parent) {
            self.fail(Error::Io);
        }
    }
    pub(super) fn publish(&self) -> u8 {
        if !sys::nonblocking(1) {
            return 125;
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut attempts = 0;
        let (kind, status) = self.error.map_or(('N', self.status), |e| ('E', e as i32));
        let header = format!(
            "MORROW_TEST 1 {kind} {status} {} {}\n",
            self.streams[0].bytes.len(),
            self.streams[1].bytes.len()
        );
        for bytes in [
            header.as_bytes(),
            &self.streams[0].bytes,
            &self.streams[1].bytes,
            b"\nMORROW_TEST_END 1\n",
        ] {
            let mut offset = 0;
            while offset < bytes.len() {
                attempts += 1;
                if attempts > BUDGET || Instant::now() >= deadline {
                    return 125;
                }
                let count = sys::write(1, &bytes[offset..bytes.len().min(offset + LIMIT)]);
                if count > 0 {
                    offset += count as usize;
                    continue;
                }
                if count < 0 && sys::errno() == libc::EINTR {
                    continue;
                }
                if count < 0 && [libc::EAGAIN, libc::EWOULDBLOCK].contains(&sys::errno()) {
                    let mut poll = libc::pollfd {
                        fd: 1,
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    if sys::poll(std::slice::from_mut(&mut poll), 10) >= 0
                        || sys::errno() == libc::EINTR
                    {
                        continue;
                    }
                }
                return 125;
            }
        }
        0
    }
}
