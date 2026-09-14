use crate::{Config, TraceEvent};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// SplitMix64, fixed algorithm/version: reproducibility does not depend on rand releases.
pub(crate) struct Random(u64);
impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0);
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^= value >> 31;
        ((u128::from(value) * u128::from(bound)) >> 64) as u64
    }
    pub fn chance(&mut self, per_mille: u16) -> bool {
        self.below(1000) < u64::from(per_mille)
    }
}
pub(crate) fn digest(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value)
        .expect("simulation data contains only serializable bounded values");
    hex(&Sha256::digest(bytes))
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    text
}
pub(crate) struct Recorder {
    pub events: u64,
    pub trace: VecDeque<TraceEvent>,
    limit: usize,
    hash: Sha256,
}
impl Recorder {
    pub fn new(config: &Config) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"morrow-sim trace v1\0");
        hash.update(digest(config));
        Self {
            events: 0,
            trace: VecDeque::new(),
            limit: config.trace_limit as usize,
            hash,
        }
    }
    pub fn record(&mut self, time_ms: u64, kind: &str, client: Option<usize>, detail: String) {
        assert!(kind.len() <= 32 && detail.len() <= 256);
        let event = TraceEvent {
            index: self.events,
            time_ms,
            kind: kind.into(),
            client: client.map(|id| id as u16),
            detail,
        };
        let bytes = serde_json::to_vec(&event).expect("bounded trace is serializable");
        self.hash.update((bytes.len() as u64).to_le_bytes());
        self.hash.update(bytes);
        self.events += 1;
        if self.limit != 0 {
            if self.trace.len() == self.limit {
                self.trace.pop_front();
            }
            self.trace.push_back(event);
        }
    }
    pub fn hash(&self) -> String {
        hex(&self.hash.clone().finalize())
    }
}

/// Checkpoint files are real, private and exclusively owned; paths never enter replay data.
pub(crate) struct Directory {
    pub path: PathBuf,
    handle: fs::File,
}
impl Directory {
    pub fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let path = std::env::temp_dir().join(format!(
                "morrow-sim-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {
                    return Ok(Self {
                        handle: fs::OpenOptions::new()
                            .read(true)
                            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                            .open(&path)?,
                        path,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::other(
            "could not reserve a private simulation directory",
        ))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if let (Ok(owned), Ok(current)) = (self.handle.metadata(), fs::symlink_metadata(&self.path))
            && current.is_dir()
            && (owned.dev(), owned.ino()) == (current.dev(), current.ino())
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
