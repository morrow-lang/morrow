use morrow_browser::*;
use morrow_web_protocol::*;

#[test]
fn offline_record_preserves_unicode_draft_without_credentials_or_replay() {
    let saved = Saved {
        draft: "Meet 🌿 offline".into(),
        snapshot: None,
        had_pending: true,
    };
    let encoded = saved.encode().unwrap();
    assert!(!encoded.contains("namespace"));
    let restored = Saved::decode(&encoded).unwrap();
    assert_eq!(restored.draft, "Meet 🌿 offline");
    assert!(restored.had_pending);
}

#[test]
fn offline_record_roundtrips_a_snapshot_with_live_viewers() {
    let saved = Saved {
        draft: "My offline draft".into(),
        snapshot: Some(Snapshot {
            version: VERSION,
            room: "garden".into(),
            incarnation: "boot".into(),
            revision: Decimal(1),
            tasks: vec![Task {
                id: Decimal(1),
                label: "Grow a lasting language".into(),
                done: false,
            }],
            viewers: Decimal(2),
        }),
        had_pending: false,
    };
    let restored = Saved::decode(&saved.encode().unwrap()).unwrap();
    assert_eq!(restored.draft, "My offline draft");
    let snapshot = restored.snapshot.unwrap();
    assert_eq!(snapshot.viewers, Decimal(2));
    assert_eq!(snapshot.tasks[0].label, "Grow a lasting language");
}

#[test]
fn an_empty_draft_does_not_erase_another_tabs_stored_draft() {
    let stored = Saved {
        draft: "My offline draft".into(),
        snapshot: None,
        had_pending: false,
    }
    .encode()
    .unwrap();
    let mut incoming = Saved {
        draft: String::new(),
        snapshot: Some(Snapshot {
            version: VERSION,
            room: "garden".into(),
            incarnation: "boot".into(),
            revision: Decimal(1),
            tasks: vec![],
            viewers: Decimal(2),
        }),
        had_pending: false,
    };
    incoming.keep_existing_draft(Some(&stored));
    assert_eq!(incoming.draft, "My offline draft");
    incoming.draft = "typed here".into();
    incoming.keep_existing_draft(Some(&stored));
    assert_eq!(incoming.draft, "typed here");
}

#[test]
fn damaged_or_oversized_storage_is_rejected_before_mount() {
    assert!(Saved::decode(&"x".repeat(MAX_SAVED_BYTES + 1)).is_err());
    assert!(
        Saved::decode(r#"{"draft":"x","snapshot":null,"had_pending":false,"access_key":"bad"}"#)
            .is_err()
    );
    let invalid = Saved {
        draft: "x".repeat(MAX_LABEL_BYTES + 1),
        snapshot: None,
        had_pending: false,
    };
    assert!(invalid.encode().is_err());
}

#[test]
fn retries_have_bounded_backoff_and_output_admission() {
    assert_eq!(reconnect_delay_ms(0, 0), 500);
    assert_eq!(reconnect_delay_ms(40, 0), 30_000);
    assert_eq!(reconnect_delay_ms(40, 999), 30_999);
    assert!(can_send(0, MAX_FRAME_BYTES));
    assert!(!can_send(MAX_OUTPUT_BYTES, 1));
    assert!(!can_send(0, MAX_FRAME_BYTES + 1));
}
