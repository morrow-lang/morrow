//! Directory-relative credential IO keeps creation and cleanup attached to owned descriptors.
use rustix::fs::{self, AtFlags, Mode, OFlags};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
};

pub(super) fn open_directory(path: &Path) -> io::Result<File> {
    fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(Into::into)
}
pub(super) fn open_child(parent: &File, name: &str) -> io::Result<File> {
    fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(Into::into)
}
pub(super) fn basename(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "credential path must be one relative basename",
        ))
    } else {
        Ok(())
    }
}
pub(super) fn read(parent: &File, name: &str, limit: usize, private: bool) -> io::Result<Vec<u8>> {
    basename(name)?;
    let mut file = File::from(fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?);
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.len() > limit as u64
        || (private && (metadata.mode() & 0o077 != 0 || metadata.nlink() != 1))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid credential file type, size or private permissions",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "credential file exceeds limit",
        ));
    }
    Ok(bytes)
}
pub(super) struct Child {
    pub name: String,
    pub directory: File,
    files: Vec<(String, File)>,
}
impl Child {
    pub fn create(parent: &File, name: String) -> io::Result<Self> {
        fs::mkdirat(parent, &name, Mode::from_bits_truncate(0o700))?;
        Ok(Self {
            name: name.clone(),
            directory: open_child(parent, &name)?,
            files: Vec::new(),
        })
    }
    pub fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        basename(name)?;
        if bytes.len() > 65_536 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "credential output exceeds limit",
            ));
        }
        let file = File::from(fs::openat(
            &self.directory,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?);
        self.files.push((name.into(), file));
        let file = &mut self.files.last_mut().unwrap().1;
        file.write_all(bytes)?;
        file.sync_all()
    }
    pub fn clean(&self) {
        for (name, file) in &self.files {
            if matches_entry(&self.directory, name.as_ref(), file) {
                let _ = fs::unlinkat(&self.directory, name, AtFlags::empty());
            }
        }
    }
}
pub(super) fn matches_entry(parent: &File, name: &OsStr, file: &File) -> bool {
    let Ok(found) = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
        return false;
    };
    let Ok(expected) = fs::fstat(file) else {
        return false;
    };
    found.st_dev == expected.st_dev && found.st_ino == expected.st_ino
}
