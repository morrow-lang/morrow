use fern_web_protocol::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Application(Arc<AtomicUsize>);
impl Domain for Application {
    fn apply(
        &mut self,
        _room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        _max_tasks: usize,
    ) -> Result<DomainChange, Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let Mutation::Add { label } = mutation else {
            return Err(Error::Malformed);
        };
        let mut tasks = current.to_vec();
        tasks.push(Task {
            id: Decimal(next_id),
            label: format!("Fern: {label}"),
            done: true,
        });
        Ok(DomainChange {
            tasks,
            next_id: next_id + 1,
            status: Status::Applied,
        })
    }
}

#[test]
fn domain_application_runs_once_after_authorization_and_revision_validation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut hub =
        Hub::with_domain("boot".into(), Limits::default(), Application(calls.clone())).unwrap();
    let connected = hub.connect("alice", "garden", None, 0).unwrap();
    let command = Command {
        version: VERSION,
        incarnation: connected.snapshot.incarnation,
        namespace: connected.namespace,
        sequence: Decimal(1),
        expected_revision: Decimal(0),
        mutation: Mutation::Add {
            label: "hello".into(),
        },
    };
    assert_eq!(
        hub.command("intruder", &connected.connection, command.clone(), 1),
        Err(Error::Unauthorized)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let outcome = hub
        .command("alice", &connected.connection, command.clone(), 2)
        .unwrap();
    assert_eq!(outcome.status, Status::Applied);
    assert_eq!(
        hub.command("alice", &connected.connection, command.clone(), 3)
            .unwrap(),
        outcome
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let snapshot = hub.snapshot("garden").unwrap();
    assert_eq!(
        snapshot.tasks,
        vec![Task {
            id: Decimal(1),
            label: "Fern: hello".into(),
            done: true
        }]
    );
    let stale = Command {
        sequence: Decimal(2),
        ..command
    };
    assert_eq!(
        hub.command("alice", &connected.connection, stale, 4)
            .unwrap()
            .status,
        Status::Conflict
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct BrokenApplication {
    resets: Arc<AtomicUsize>,
    reset_fails: bool,
    invalid_output: bool,
}
impl Domain for BrokenApplication {
    fn apply(
        &mut self,
        _room: &str,
        _current: &[Task],
        _next_id: i64,
        _mutation: &Mutation,
        _max_tasks: usize,
    ) -> Result<DomainChange, Error> {
        // A stateful actor may fail after beginning a transition; no old namespace
        // may carry on as if the gateway's last confirmed copy is authoritative.
        if self.invalid_output {
            Ok(DomainChange {
                tasks: vec![],
                next_id: -1,
                status: Status::Applied,
            })
        } else {
            Err(Error::Malformed)
        }
    }
    fn reset(&mut self, _room: &str) -> Result<(), Error> {
        self.resets.fetch_add(1, Ordering::SeqCst);
        if self.reset_fails {
            Err(Error::Malformed)
        } else {
            Ok(())
        }
    }
}

#[test]
fn a_domain_fault_invalidates_its_incarnation_and_a_failed_reset_closes_the_room() {
    for (reset_fails, invalid_output) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let resets = Arc::new(AtomicUsize::new(0));
        let mut hub = Hub::with_domain(
            "boot".into(),
            Limits::default(),
            BrokenApplication {
                resets: resets.clone(),
                reset_fails,
                invalid_output,
            },
        )
        .unwrap();
        let connected = hub.connect("alice", "garden", None, 0).unwrap();
        let command = Command {
            version: VERSION,
            incarnation: connected.snapshot.incarnation.clone(),
            namespace: connected.namespace,
            sequence: Decimal(1),
            expected_revision: Decimal(0),
            mutation: Mutation::Add {
                label: "might have applied".into(),
            },
        };
        assert_eq!(
            hub.command("alice", &connected.connection, command.clone(), 1),
            Err(Error::Malformed)
        );
        assert_eq!(resets.load(Ordering::SeqCst), 1);
        if reset_fails {
            assert_eq!(hub.snapshot("garden"), Err(Error::ResyncRequired));
            assert_eq!(
                hub.connect("bob", "garden", None, 2),
                Err(Error::ResyncRequired)
            );
            assert_eq!(
                hub.command("alice", &connected.connection, command, 3),
                Err(Error::ResyncRequired)
            );
            assert!(!hub.connection_is_live(&connected.connection, 3));
        } else {
            let snapshot = hub.snapshot("garden").unwrap();
            assert_ne!(snapshot.incarnation, connected.snapshot.incarnation);
            assert_eq!(snapshot.revision, Decimal(0));
            assert!(snapshot.tasks.is_empty());
            assert_eq!(
                hub.command("alice", &connected.connection, command, 2),
                Err(Error::IncarnationMismatch)
            );
        }
    }
}

struct CheckpointApplication;
impl Domain for CheckpointApplication {
    fn restore(&mut self, _room: &str) -> Result<Option<DomainChange>, Error> {
        Ok(Some(DomainChange {
            tasks: vec![Task {
                id: Decimal(42),
                label: "committed before restart 🌱".into(),
                done: true,
            }],
            next_id: 43,
            status: Status::Applied,
        }))
    }
    fn apply(
        &mut self,
        _room: &str,
        _current: &[Task],
        _next_id: i64,
        _mutation: &Mutation,
        _max_tasks: usize,
    ) -> Result<DomainChange, Error> {
        Err(Error::Malformed)
    }
}

#[test]
fn restart_and_domain_recovery_restore_committed_tasks_with_fresh_delivery_identity() {
    let mut hub =
        Hub::with_domain("new-boot".into(), Limits::default(), CheckpointApplication).unwrap();
    let first = hub.connect("alice", "garden", None, 0).unwrap();
    assert_eq!(
        first.snapshot.tasks,
        vec![Task {
            id: Decimal(42),
            label: "committed before restart 🌱".into(),
            done: true
        }]
    );
    assert_eq!(first.snapshot.revision, Decimal(0));
    let command = Command {
        version: VERSION,
        incarnation: first.snapshot.incarnation.clone(),
        namespace: first.namespace,
        sequence: Decimal(1),
        expected_revision: Decimal(0),
        mutation: Mutation::Remove { id: Decimal(42) },
    };
    assert_eq!(
        hub.command("alice", &first.connection, command.clone(), 1),
        Err(Error::Malformed)
    );
    let recovered = hub.snapshot("garden").unwrap();
    assert_eq!(recovered.tasks, first.snapshot.tasks);
    assert_eq!(recovered.revision, Decimal(0));
    assert_ne!(recovered.incarnation, first.snapshot.incarnation);
    assert_eq!(
        hub.command("alice", &first.connection, command, 2),
        Err(Error::IncarnationMismatch)
    );
}
