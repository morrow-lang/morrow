use crate::{Message, protocol::*};
pub fn fixtures() -> Vec<(String, Message)> {
    let mut out = Vec::new();
    for resume in [None, Some("namespace-1".into())] {
        out.push((
            format!("join-resume-{}", resume.is_some()),
            Message::Client(ClientMessage::Join {
                room: "room-🌿".into(),
                resume_namespace: resume,
            }),
        ));
    }
    for (name, mutation) in [
        (
            "add",
            Mutation::Add {
                label: "Learn Morrow 🌿".into(),
            },
        ),
        (
            "set",
            Mutation::SetDone {
                id: Decimal(9007199254740993),
                done: false,
            },
        ),
        (
            "remove",
            Mutation::Remove {
                id: Decimal(i64::MAX),
            },
        ),
    ] {
        out.push((
            format!("command-{name}"),
            Message::Client(ClientMessage::Command(Command {
                version: VERSION,
                incarnation: "boot-1/room-1".into(),
                namespace: "namespace-1".into(),
                sequence: Decimal(42),
                expected_revision: Decimal(9007199254740993),
                mutation,
            })),
        ));
    }
    for count in [0, 1, 10, 100] {
        for (labels, label) in [
            ("ascii", "Learn Morrow".to_string()),
            ("unicode", "こんにちは 🌿".repeat(10)),
            ("escaped", "\\".repeat(256)),
        ] {
            let snapshot = Snapshot {
                version: VERSION,
                room: "room-🌿".into(),
                incarnation: "boot-1/room-1".into(),
                revision: Decimal(9007199254740993),
                tasks: (0..count)
                    .map(|i| Task {
                        id: Decimal(i64::MAX - i),
                        label: label.clone(),
                        done: i % 2 == 0,
                    })
                    .collect(),
            };
            out.push((
                format!("snapshot-{count}-{labels}"),
                Message::Server(ServerMessage::Snapshot(snapshot.clone())),
            ));
            if count == 1 && labels == "ascii" {
                out.push((
                    "reset".into(),
                    Message::Server(ServerMessage::Reset(snapshot.clone())),
                ));
                out.push((
                    "connected".into(),
                    Message::Server(ServerMessage::Connected(Connected {
                        version: VERSION,
                        connection: "connection-1".into(),
                        namespace: "namespace-1".into(),
                        next_sequence: Decimal(43),
                        snapshot,
                        resumed: true,
                    })),
                ));
            }
        }
    }
    for status in [
        Status::Applied,
        Status::Conflict,
        Status::NotFound,
        Status::Capacity,
        Status::Unknown,
    ] {
        out.push((
            format!("outcome-{status:?}"),
            Message::Server(ServerMessage::Outcome(Outcome {
                version: VERSION,
                incarnation: "boot-1/room-1".into(),
                namespace: "namespace-1".into(),
                sequence: Decimal(42),
                revision: Decimal(9007199254740993),
                status,
            })),
        ));
    }
    for error in [
        Error::Malformed,
        Error::FrameTooLarge,
        Error::VersionMismatch,
        Error::InvalidIdentity,
        Error::InvalidLabel,
        Error::InvalidLimits,
        Error::RoomLimit,
        Error::NamespaceLimit,
        Error::ConnectionLimit,
        Error::NamespaceExpired,
        Error::ConnectionExpired,
        Error::Unauthorized,
        Error::IncarnationMismatch,
        Error::PayloadMismatch,
        Error::SequenceGap,
        Error::Exhausted,
        Error::TimeRegression,
        Error::Offline,
        Error::Pending,
        Error::ResyncRequired,
        Error::UnexpectedOutcome,
        Error::StaleSnapshot,
    ] {
        out.push((
            format!("error-{error:?}"),
            Message::Server(ServerMessage::Error(error)),
        ));
    }
    out
}
