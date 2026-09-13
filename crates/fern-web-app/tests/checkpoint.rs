use fern_web_app::NativeDomain;
use fern_web_protocol::{Domain, Mutation};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fern-checkpoint-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn acknowledged_native_state_is_recovered_and_one_writer_owns_the_directory() {
    let directory = Directory::new();
    let mut first = NativeDomain::persistent(&directory.0).unwrap();
    assert!(
        NativeDomain::persistent(&directory.0).is_err(),
        "a second writer must not race checkpoints"
    );
    let changed = first
        .apply(
            "garden",
            &[],
            1,
            &Mutation::Add {
                label: "durable 🌱".into(),
            },
            100,
        )
        .unwrap();
    drop(first);
    let mut restored = NativeDomain::persistent(&directory.0).unwrap();
    let checkpoint = restored.restore("garden").unwrap().unwrap();
    assert_eq!(checkpoint.tasks, changed.tasks);
    assert_eq!(checkpoint.next_id, 2);
    let applied = restored
        .apply(
            "garden",
            &checkpoint.tasks,
            2,
            &Mutation::Add {
                label: "second".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(applied.tasks.len(), 2);
    restored.reset("garden").unwrap();
    assert_eq!(
        restored.restore("garden").unwrap().unwrap().tasks,
        applied.tasks,
        "actor recovery preserves acknowledged checkpoint"
    );
}

#[test]
fn corrupt_future_or_symlink_checkpoints_fail_closed() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("rooms.json"),
        b"{\"version\":999,\"rooms\":{}}",
    )
    .unwrap();
    assert!(NativeDomain::persistent(&directory.0).is_err());
    fs::write(directory.0.join("rooms.json"), b"{broken").unwrap();
    assert!(NativeDomain::persistent(&directory.0).is_err());
    fs::remove_file(directory.0.join("rooms.json")).unwrap();
    #[cfg(unix)]
    {
        let outside = directory.0.join("outside");
        fs::write(&outside, b"do not overwrite").unwrap();
        std::os::unix::fs::symlink(&outside, directory.0.join("rooms.json")).unwrap();
        assert!(NativeDomain::persistent(&directory.0).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"do not overwrite");
    }
}

#[test]
fn replacement_directory_and_lock_never_receive_an_old_owners_checkpoint() {
    let directory = Directory::new();
    let path = directory.0.join("state");
    let mut domain = NativeDomain::persistent(&path).unwrap();
    fs::rename(&path, directory.0.join("retained")).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("rooms.json"), b"foreign replacement").unwrap();
    assert!(
        domain
            .apply(
                "garden",
                &[],
                1,
                &Mutation::Add {
                    label: "wrong directory".into()
                },
                100
            )
            .is_err()
    );
    assert_eq!(
        fs::read(path.join("rooms.json")).unwrap(),
        b"foreign replacement"
    );
    drop(domain);

    let path = directory.0.join("other");
    let mut domain = NativeDomain::persistent(&path).unwrap();
    fs::remove_file(path.join("owner.lock")).unwrap();
    fs::write(path.join("owner.lock"), b"new owner").unwrap();
    assert!(
        domain
            .apply(
                "garden",
                &[],
                1,
                &Mutation::Add {
                    label: "lost lock".into()
                },
                100
            )
            .is_err()
    );
    assert!(!path.join("rooms.json").exists());
    assert_eq!(fs::read(path.join("owner.lock")).unwrap(), b"new owner");
}

#[test]
fn duplicate_room_keys_are_rejected_instead_of_silently_losing_a_checkpoint() {
    let directory = Directory::new();
    fs::write(directory.0.join("rooms.json"), br#"{"version":1,"rooms":{"garden":{"tasks":[],"next_id":1},"garden":{"tasks":[],"next_id":2}}}"#).unwrap();
    assert!(NativeDomain::persistent(&directory.0).is_err());
}

#[test]
fn restored_full_width_identity_stays_exact_through_the_native_actor_and_checkpoint() {
    let directory = Directory::new();
    fs::write(directory.0.join("rooms.json"), br#"{"version":1,"rooms":{"garden":{"tasks":[{"id":"9007199254740993","label":"existing","done":false}],"next_id":9007199254740994}}}"#).unwrap();
    let mut domain = NativeDomain::persistent(&directory.0).unwrap();
    let checkpoint = domain.restore("garden").unwrap().unwrap();
    assert_eq!(checkpoint.tasks[0].id.0, 9_007_199_254_740_993);
    assert_eq!(checkpoint.next_id, 9_007_199_254_740_994);
    let changed = domain
        .apply(
            "garden",
            &checkpoint.tasks,
            checkpoint.next_id,
            &Mutation::Add {
                label: "exact new identity".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(changed.tasks[1].id.0, 9_007_199_254_740_994);
    assert_eq!(changed.next_id, 9_007_199_254_740_995);
    drop(domain);
    let mut reopened = NativeDomain::persistent(&directory.0).unwrap();
    let checkpoint = reopened.restore("garden").unwrap().unwrap();
    assert_eq!(checkpoint.tasks, changed.tasks);
    assert_eq!(checkpoint.next_id, 9_007_199_254_740_995);
}
