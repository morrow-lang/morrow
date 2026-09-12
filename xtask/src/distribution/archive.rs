//! Bounded streaming archive format. No extraction, extension headers, or link entries.
use super::{Dir, FILE_LIMIT, Input, OPTIONAL, REQUIRED, TOTAL_LIMIT, executable, validate_marker};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek},
    path::Path,
};
/// Compute SHA-256 over at most the permitted compressed archive size.
pub(super) fn hash(file: &mut File) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > TOTAL_LIMIT {
            return Err("release archive exceeds byte limit".into());
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
struct Count<'a> {
    file: &'a mut File,
    bytes: u64,
}
impl Read for Count<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(buffer)?;
        self.bytes += count as u64;
        Ok(count)
    }
}
/// Produce deterministic USTAR headers and one gzip stream from held staged files.
pub(super) fn write(destination: &mut File, stem: &str, files: &mut [Input]) -> Result<(), String> {
    let gzip = flate2::GzBuilder::new()
        .mtime(0)
        .write(destination, flate2::Compression::default());
    let mut archive = tar::Builder::new(gzip);
    for input in files {
        let mut header = tar::Header::new_ustar();
        header
            .set_path(format!("{stem}/{}", input.name))
            .map_err(|e| e.to_string())?;
        header.set_size(input.size);
        header.set_mode(input.mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        let mut reader = Count {
            file: &mut input.file,
            bytes: 0,
        };
        archive
            .append(&header, (&mut reader).take(input.size + 1))
            .map_err(|e| e.to_string())?;
        if reader.bytes != input.size {
            return Err(format!(
                "staging component changed while archiving: {}",
                input.name
            ));
        }
    }
    archive
        .into_inner()
        .map_err(|e| e.to_string())?
        .finish()
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn open(path: &Path, limit: u64) -> Result<File, String> {
    let directory = Dir::open(path.parent().ok_or("missing archive directory")?, false)
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid archive filename")?;
    let file = directory.file(name).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > limit {
        return Err("archive/checksum exceeds bounded regular-file limit".into());
    }
    Ok(file)
}
pub(super) fn verify(path: &Path, checksum: &Path) -> Result<(), String> {
    let mut file = open(path, TOTAL_LIMIT)?;
    let mut checksum_file = open(checksum, 4096)?;
    let mut text = String::new();
    checksum_file
        .by_ref()
        .take(4097)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid archive filename")?;
    let expected = format!("{}  {name}\n", hash(&mut file)?);
    if text != expected {
        return Err("archive checksum mismatch or invalid checksum record".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    verify_contents(file)
}
/// Read only raw regular members under one safe root, with exact length accounting.
/// Reject extra gzip members/trailing payloads, including hidden post-tar entries.
pub(super) fn verify_contents(file: File) -> Result<(), String> {
    let gzip = flate2::bufread::GzDecoder::new(BufReader::new(file));
    let mut archive = tar::Archive::new(gzip.take(TOTAL_LIMIT + 65537));
    let mut seen = BTreeSet::new();
    let mut root = None;
    let mut total = 0u64;
    let mut tar_bytes = 0u64;
    let entries = archive.entries().map_err(|e| e.to_string())?.raw(true);
    for item in entries {
        let mut item = item.map_err(|e| e.to_string())?;
        if seen.len() >= REQUIRED.len() + OPTIONAL.len() {
            return Err("archive has too many members".into());
        }
        let path = item.path_bytes();
        if path.len() > 256 {
            return Err("archive member name exceeds limit".into());
        }
        let path = std::str::from_utf8(&path).map_err(|_| "archive name must be UTF-8")?;
        let (prefix, name) = path.split_once('/').ok_or("archive member lacks a root")?;
        if !prefix.starts_with("fern-")
            || prefix.len() > 192
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
            || !REQUIRED.contains(&name) && !OPTIONAL.contains(&name)
            || !seen.insert(name.to_owned())
        {
            return Err("archive has unexpected or duplicate members".into());
        }
        if root.as_ref().is_some_and(|old| old != prefix) {
            return Err("archive has multiple roots".into());
        }
        root = Some(prefix.to_owned());
        let name = name.to_owned();
        let header = item.header();
        let size = header.size().map_err(|e| e.to_string())?;
        if header.entry_type() != tar::EntryType::Regular || size == 0 || size > FILE_LIMIT {
            return Err("archive component must be a bounded regular file".into());
        }
        if header.mode().map_err(|e| e.to_string())?
            != if executable(&name) { 0o755 } else { 0o644 }
        {
            return Err(format!("archive component has invalid permissions: {name}"));
        }
        tar_bytes += 512 + size.div_ceil(512) * 512;
        total = total.checked_add(size).ok_or("archive size overflow")?;
        if total > TOTAL_LIMIT {
            return Err("archive exceeds aggregate component limit".into());
        }
        if name == "fern-package.json" {
            let mut marker = Vec::new();
            item.by_ref()
                .take(65537)
                .read_to_end(&mut marker)
                .map_err(|e| e.to_string())?;
            validate_marker(&marker)?;
            if marker.len() as u64 != size {
                return Err("truncated package marker".into());
            }
        } else {
            let count = io::copy(&mut item, &mut io::sink()).map_err(|e| e.to_string())?;
            if count != size {
                return Err("truncated archive component".into());
            }
        }
    }
    if REQUIRED.iter().any(|name| !seen.contains(*name)) {
        return Err("archive missing required members".into());
    }
    // Tar stops at its zero trailer. Consume and validate the bounded remainder
    // so gzip CRC errors and concatenated/hidden data cannot be silently ignored.
    let mut tail = archive.into_inner();
    let mut buffer = [0; 4096];
    let mut padding = 0u64;
    if TOTAL_LIMIT + 65537 - tail.limit() != tar_bytes + 512 {
        return Err("archive lacks complete tar end blocks".into());
    }
    loop {
        let count = tail.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        padding += count as u64;
        if buffer[..count].iter().any(|&b| b != 0) {
            return Err("archive has trailing tar data".into());
        }
    }
    if padding < 512 || !padding.is_multiple_of(512) {
        return Err("archive lacks complete tar end blocks".into());
    }
    if tail.limit() == 0 {
        return Err("archive exceeds decompression limit".into());
    }
    let mut compressed = tail.into_inner().into_inner();
    if !compressed.fill_buf().map_err(|e| e.to_string())?.is_empty() {
        return Err("archive has trailing compressed data".into());
    }
    Ok(())
}
