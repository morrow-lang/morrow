//! Protect source files when replacing a previously built preview executable.
use std::{fs, io::Read, path::Path};

/// An absent destination is allowed. Existing files must be native executable
/// artifacts; an executable permission bit alone also admits source scripts.
pub fn validate(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Err("web-build destination is not a regular executable file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("web-build refuses to overwrite a non-executable destination".into());
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("web-build destination changed to a non-file".into());
    }
    let mut magic = [0; 4];
    file.read_exact(&mut magic)
        .map_err(|_| "web-build destination is not a native executable")?;
    if !matches!(
        &magic,
        b"\x7fELF"
            | b"\xcf\xfa\xed\xfe"
            | b"\xfe\xed\xfa\xcf"
            | b"\xca\xfe\xba\xbe"
            | b"\xbe\xba\xfe\xca"
    ) {
        return Err("web-build refuses to overwrite source or non-native artifacts".into());
    }
    Ok(())
}
