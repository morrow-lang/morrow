//! Single-writer, bounded atomic room checkpoints, committed before acknowledgement.
use fern_web_protocol::{DomainChange, Error, Status, Task};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const MAX_BYTES: usize = 8 * 1024 * 1024;
type Identity = (u64, u64);
fn identity(file: &File) -> io::Result<Identity> {
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub tasks: Vec<Task>,
    pub next_id: i64,
}
impl State {
    pub(super) fn change(&self) -> DomainChange {
        DomainChange {
            tasks: self.tasks.clone(),
            next_id: self.next_id,
            status: Status::Applied,
        }
    }
    pub(super) fn native_json(&self) -> Result<String, Error> {
        let tasks: Vec<_> = self
            .tasks
            .iter()
            .map(|task| serde_json::json!({"id":task.id.0,"label":task.label,"done":task.done}))
            .collect();
        serde_json::to_string(&serde_json::json!({"tasks":tasks,"next_id":self.next_id}))
            .map_err(|_| Error::Malformed)
    }
    fn validate(&self) -> io::Result<()> {
        let mut ids = BTreeSet::new();
        if self.next_id < 1
            || self.tasks.len() > 100
            || self.tasks.iter().any(|task| {
                task.id.0 < 1
                    || task.id.0 >= self.next_id
                    || !ids.insert(task.id)
                    || task.label.trim().is_empty()
                    || task.label.len() > 256
                    || task.label.chars().any(char::is_control)
            })
        {
            return Err(io::Error::other("invalid room checkpoint"));
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u8,
    #[serde(deserialize_with = "unique_rooms")]
    rooms: BTreeMap<String, State>,
}
fn unique_rooms<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, State>, D::Error> {
    struct Rooms;
    impl<'de> Visitor<'de> for Rooms {
        type Value = BTreeMap<String, State>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("at most 128 unique room checkpoints")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut rooms = BTreeMap::new();
            while let Some(name) = map.next_key::<String>()? {
                if rooms.len() == 128 || rooms.contains_key(&name) {
                    return Err(serde::de::Error::custom("duplicate room or room limit"));
                }
                rooms.insert(name, map.next_value()?);
            }
            Ok(rooms)
        }
    }
    deserializer.deserialize_map(Rooms)
}

pub(super) struct Store {
    path: PathBuf,
    directory: File,
    lock: File,
    // Retain the inode itself, so unlink/recreate cannot exploit inode reuse.
    checkpoint: Option<File>,
    rooms: BTreeMap<String, State>,
}

fn open_at(directory: &File, name: &CStr, flags: i32) -> io::Result<File> {
    // SAFETY: retained directory FD and NUL-terminated basename remain live. New
    // descriptor ownership transfers exactly once to File; final symlinks and
    // blocking special files are never followed while inspecting replacements.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor on the successful path.
    let file = unsafe { File::from_raw_fd(descriptor) };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("checkpoint entry is not a regular file"));
    }
    Ok(file)
}
fn entry_identity(directory: &File, name: &CStr) -> io::Result<Option<Identity>> {
    match open_at(directory, name, libc::O_RDONLY) {
        Ok(file) => identity(&file).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

struct Temporary<'a> {
    directory: &'a File,
    name: CString,
    file: File,
    published: bool,
}
impl Temporary<'_> {
    fn owns_entry(&self) -> io::Result<bool> {
        Ok(entry_identity(self.directory, &self.name)? == Some(identity(&self.file)?))
    }
    fn publish(&mut self) -> io::Result<()> {
        if !self.owns_entry()? {
            return Err(io::Error::other("checkpoint temporary was replaced"));
        }
        // SAFETY: both names are basenames in the pinned, single-writer directory;
        // the temporary was exclusively created and its identity was checked.
        if unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
                self.directory.as_raw_fd(),
                c"rooms.json".as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        self.published = true;
        Ok(())
    }
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        if !self.published && self.owns_entry().unwrap_or(false) {
            // SAFETY: unlink only the still-owned basename inside the retained
            // private writer directory, without traversing a replacement parent.
            unsafe {
                libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
            }
        }
    }
}

impl Store {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        Self::open_scoped(path, None)
    }
    pub(super) fn open_scoped(path: &Path, placement: Option<&str>) -> io::Result<Self> {
        if placement.is_some_and(|value| {
            value.is_empty()
                || value.len() > 256
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid checkpoint placement identity",
            ));
        }
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let metadata = directory.metadata()?;
        if metadata.mode() & 0o022 != 0 {
            return Err(io::Error::other(
                "checkpoint directory must not be writable by group or others",
            ));
        }
        let path = path.canonicalize()?;
        let lock = open_at(&directory, c"owner.lock", libc::O_RDWR | libc::O_CREAT)?;
        lock.try_lock().map_err(io::Error::other)?;
        let (rooms, checkpoint) = match open_at(&directory, c"rooms.json", libc::O_RDONLY) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (BTreeMap::new(), None),
            Err(error) => return Err(error),
            Ok(mut file) => {
                let mut bytes = Vec::new();
                Read::by_ref(&mut file)
                    .take(MAX_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > MAX_BYTES {
                    return Err(io::Error::other("checkpoint exceeds 8 MiB"));
                }
                let snapshot: Snapshot =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if snapshot.version != 1 {
                    return Err(io::Error::other("unsupported checkpoint version"));
                }
                for (room, state) in &snapshot.rooms {
                    if room.is_empty() || room.len() > 128 || room.chars().any(char::is_control) {
                        return Err(io::Error::other("invalid checkpoint room identity"));
                    }
                    state.validate()?;
                }
                (snapshot.rooms, Some(file))
            }
        };
        let mut store = Self {
            path,
            directory,
            lock,
            checkpoint,
            rooms,
        };
        store.check_ownership()?;
        store.bind_placement(placement)?;
        Ok(store)
    }
    fn bind_placement(&mut self, placement: Option<&str>) -> io::Result<()> {
        let mut stored = String::new();
        Read::by_ref(&mut self.lock)
            .take(513)
            .read_to_string(&mut stored)?;
        if stored.len() > 512 {
            return Err(io::Error::other(
                "checkpoint placement metadata exceeds limit",
            ));
        }
        let requested =
            placement.map(|identity| format!("fern-cluster-placement-v1\n{identity}\n"));
        match requested {
            None if stored.is_empty() => Ok(()),
            Some(requested) if stored == requested => Ok(()),
            Some(requested) if stored.is_empty() && self.rooms.is_empty() => {
                // The existing retained file carries the exclusive writer lock.
                // Pin placement before any room can be created or acknowledged.
                self.check_ownership()?;
                self.lock.write_all(requested.as_bytes())?;
                self.lock.sync_all()?;
                self.directory.sync_all()?;
                self.check_ownership()
            }
            _ => Err(io::Error::other(
                "checkpoint placement differs; explicit offline migration is required",
            )),
        }
    }
    fn check_ownership(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.path)?;
        if !metadata.is_dir()
            || (metadata.dev(), metadata.ino()) != identity(&self.directory)?
            || entry_identity(&self.directory, c"owner.lock")? != Some(identity(&self.lock)?)
            || entry_identity(&self.directory, c"rooms.json")?
                != self.checkpoint.as_ref().map(identity).transpose()?
        {
            return Err(io::Error::other(
                "checkpoint directory, lock or state was replaced",
            ));
        }
        Ok(())
    }
    pub(super) fn get(&self, room: &str) -> Option<&State> {
        self.rooms.get(room)
    }
    pub(super) fn commit(&mut self, room: &str, change: &DomainChange) -> io::Result<()> {
        self.commit_with_sync(room, change, File::sync_all)
    }
    fn commit_with_sync(
        &mut self,
        room: &str,
        change: &DomainChange,
        sync_directory: impl FnOnce(&File) -> io::Result<()>,
    ) -> io::Result<()> {
        self.check_ownership()?;
        let state = State {
            tasks: change.tasks.clone(),
            next_id: change.next_id,
        };
        state.validate()?;
        let mut rooms = self.rooms.clone();
        rooms.insert(room.into(), state);
        if rooms.len() > 128 {
            return Err(io::Error::other("checkpoint room limit"));
        }
        let bytes =
            serde_json::to_vec(&Snapshot { version: 1, rooms }).map_err(io::Error::other)?;
        if bytes.len() > MAX_BYTES {
            return Err(io::Error::other("checkpoint exceeds 8 MiB"));
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = CString::new(format!(
            ".rooms-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .map_err(io::Error::other)?;
        let file = open_at(
            &self.directory,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        let mut temporary = Temporary {
            directory: &self.directory,
            name,
            file,
            published: false,
        };
        temporary.file.write_all(&bytes)?;
        temporary.file.sync_all()?;
        let checkpoint = temporary.file.try_clone()?;
        self.check_ownership()?;
        temporary.publish()?;
        // A failed directory fsync is uncertain: the new file may be durable.
        // Remember its identity but retain the last acknowledged in-memory state;
        // gateway recovery resets incarnation instead of reporting false success.
        self.checkpoint = Some(checkpoint);
        sync_directory(&self.directory)?;
        self.rooms.insert(
            room.into(),
            State {
                tasks: change.tasks.clone(),
                next_id: change.next_id,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_web_protocol::Decimal;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "fern-checkpoint-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cluster_placement_is_persistent_and_cannot_be_silently_changed() {
        let directory = Directory::new();
        drop(Store::open_scoped(&directory.0, Some("cluster-a/node-a/manifest-one")).unwrap());
        assert!(Store::open(&directory.0).is_err());
        assert!(Store::open_scoped(&directory.0, Some("cluster-a/node-b/manifest-one")).is_err());
        assert!(Store::open_scoped(&directory.0, Some("cluster-a/node-a/manifest-two")).is_err());
        assert!(Store::open_scoped(&directory.0, Some("cluster-a/node-a/manifest-one")).is_ok());
    }

    #[test]
    fn cluster_placement_cannot_adopt_existing_unscoped_room_state() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.0).unwrap();
        store
            .commit(
                "room",
                &DomainChange {
                    tasks: vec![Task {
                        id: Decimal(1),
                        label: "preserve me".into(),
                        done: false,
                    }],
                    next_id: 2,
                    status: Status::Applied,
                },
            )
            .unwrap();
        drop(store);
        assert!(Store::open_scoped(&directory.0, Some("cluster-a/node-a/manifest-one")).is_err());
        assert_eq!(
            Store::open(&directory.0)
                .unwrap()
                .get("room")
                .unwrap()
                .tasks[0]
                .label,
            "preserve me"
        );
    }

    #[test]
    fn temporary_cleanup_preserves_a_foreign_replacement() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let name = CString::new(".owned.tmp").unwrap();
        let file = open_at(
            &store.directory,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        let temporary = Temporary {
            directory: &store.directory,
            name,
            file,
            published: false,
        };
        fs::rename(directory.0.join(".owned.tmp"), directory.0.join("retained")).unwrap();
        fs::write(directory.0.join(".owned.tmp"), b"foreign").unwrap();
        drop(temporary);
        assert_eq!(
            fs::read(directory.0.join(".owned.tmp")).unwrap(),
            b"foreign"
        );
        assert!(directory.0.join("retained").exists());
    }

    #[test]
    fn directory_sync_failure_reports_uncertainty_without_losing_acknowledged_state() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.0).unwrap();
        let first = DomainChange {
            tasks: vec![Task {
                id: Decimal(1),
                label: "acknowledged".into(),
                done: false,
            }],
            next_id: 2,
            status: Status::Applied,
        };
        store.commit("garden", &first).unwrap();
        let mut tasks = first.tasks.clone();
        tasks.push(Task {
            id: Decimal(2),
            label: "uncertain".into(),
            done: false,
        });
        let next = DomainChange {
            tasks,
            next_id: 3,
            status: Status::Applied,
        };
        assert!(
            store
                .commit_with_sync("garden", &next, |_| Err(io::Error::other(
                    "injected directory sync failure"
                )))
                .is_err()
        );
        assert_eq!(store.get("garden").unwrap().tasks, first.tasks);
        store.check_ownership().unwrap();
        drop(store);
        let reopened = Store::open(&directory.0).unwrap();
        // rename completed before the injected failure. Recovery may observe this
        // unacknowledged update; a fresh incarnation prevents automatic replay.
        assert_eq!(reopened.get("garden").unwrap().tasks, next.tasks);
        assert_eq!(reopened.get("garden").unwrap().tasks[0], first.tasks[0]);
    }
}
