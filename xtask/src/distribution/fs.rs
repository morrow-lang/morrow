//! Directory-relative filesystem boundary: never follow input or destination links.
use std::{
    ffi::{CString, OsStr},
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
pub(super) struct Dir(File);
fn name(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.contains(&b'/') || bytes == b"." || bytes == b".." {
        return Err(io::Error::other("invalid component name"));
    }
    CString::new(bytes).map_err(io::Error::other)
}
fn owned(fd: i32) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful open returned exclusive ownership of this descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
impl Dir {
    /// Open each component relative to its held parent; optional creation uses mkdirat.
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Self> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        // SAFETY: constant terminated path and valid flags; owned records descriptor ownership.
        let mut directory = Self(owned(unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?);
        for component in absolute.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(child) => directory = directory.child(child, create)?,
                _ => {
                    return Err(io::Error::other(
                        "parent traversal is not a distribution path",
                    ));
                }
            }
        }
        Ok(directory)
    }
    pub(super) fn child(&self, child: &OsStr, create: bool) -> io::Result<Self> {
        let name = name(child)?;
        if create {
            // SAFETY: held parent and valid terminated single-component name.
            if unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o755) } < 0
                && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
            {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: held parent and valid name; O_NOFOLLOW rejects a replaced symlink.
        owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK,
            )
        })
        .map(Self)
    }
    /// Open a regular candidate without blocking on FIFOs or following symlinks.
    pub(super) fn file(&self, child: &str) -> io::Result<File> {
        let name = name(OsStr::new(child))?;
        // SAFETY: name and directory remain alive; descriptor ownership transfers once.
        let file = owned(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        })?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("component must be a regular file"));
        }
        Ok(file)
    }
    pub(super) fn metadata(&self, child: &str) -> io::Result<Option<libc::stat>> {
        let name = name(OsStr::new(child))?;
        let mut value = std::mem::MaybeUninit::uninit();
        // SAFETY: output points to valid uninitialized stat storage, read only on success.
        if unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            Ok(Some(unsafe { value.assume_init() }))
        } else {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
    /// Preflight a destination without dereferencing it. Only missing or regular names qualify.
    pub(super) fn destination(&self, child: &str) -> io::Result<()> {
        if self
            .metadata(child)?
            .is_some_and(|info| info.st_mode & libc::S_IFMT != libc::S_IFREG)
        {
            return Err(io::Error::other(format!(
                "invalid install destination: {child}"
            )));
        }
        Ok(())
    }
    pub(super) fn rename(&self, source: &str, to: &Dir, destination: &str) -> io::Result<()> {
        let source = name(OsStr::new(source))?;
        let destination = name(OsStr::new(destination))?;
        // SAFETY: both names are single components anchored to live directory descriptors.
        if unsafe {
            libc::renameat(
                self.0.as_raw_fd(),
                source.as_ptr(),
                to.0.as_raw_fd(),
                destination.as_ptr(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    pub(super) fn unlink(&self, child: &str, directory: bool) -> io::Result<()> {
        let name = name(OsStr::new(child))?;
        // SAFETY: unlinkat affects only this directory-relative name, without traversal.
        if unsafe {
            libc::unlinkat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
pub(super) struct Temporary {
    pub dir: Dir,
    parent: Dir,
    name: String,
    files: Vec<String>,
}
impl Temporary {
    /// Exclusively allocate one private directory; every produced name stays beneath its fd.
    pub(super) fn new(parent: &Dir) -> io::Result<Self> {
        for _ in 0..100 {
            let child = format!(
                ".morrow-dist-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let name = name(OsStr::new(&child))?;
            // SAFETY: held parent, exclusive single-component create and restrictive mode.
            if unsafe { libc::mkdirat(parent.0.as_raw_fd(), name.as_ptr(), 0o700) } == 0 {
                return Ok(Self {
                    dir: parent.child(OsStr::new(&child), false)?,
                    parent: Dir(parent.0.try_clone()?),
                    name: child,
                    files: Vec::new(),
                });
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
        Err(io::Error::other(
            "cannot allocate private distribution directory",
        ))
    }
    pub(super) fn create(&mut self, child: &str) -> io::Result<File> {
        let name = name(OsStr::new(child))?;
        // SAFETY: exclusive creation in the owned private directory; fd is adopted once.
        let file = owned(unsafe {
            libc::openat(
                self.dir.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        })?;
        self.files.push(child.into());
        Ok(file)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        for name in &self.files {
            let _ = self.dir.unlink(name, false);
        }
        if let (Ok(Some(named)), Ok(held)) =
            (self.parent.metadata(&self.name), self.dir.0.metadata())
        {
            #[allow(clippy::unnecessary_cast)] // Darwin dev_t is signed i32; Linux uses u64.
            if named.st_dev as u64 == held.dev() && named.st_ino == held.ino() {
                let _ = self.parent.unlink(&self.name, true);
            }
        }
    }
}
