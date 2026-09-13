use super::*;
use fern_web_protocol::{Command, Decimal, DomainChange, Mutation, Status, Task};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.open.lock().unwrap() = true;
        self.0.changed.notify_all();
    }
}
struct Application {
    worker: usize,
    thread: std::thread::ThreadId,
    local: Rc<Cell<usize>>,
    entered: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    calls: Arc<AtomicUsize>,
    gate: Arc<Gate>,
}
impl Domain for Application {
    fn apply(
        &mut self,
        _room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        _: usize,
    ) -> Result<DomainChange, Error> {
        assert_eq!(std::thread::current().id(), self.thread);
        self.local.set(self.local.get() + 1);
        if self.worker == 0 {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            let mut open = self.gate.open.lock().unwrap();
            while !*open {
                open = self.gate.changed.wait(open).unwrap();
            }
        }
        let Mutation::Add { label } = mutation else {
            return Err(Error::Malformed);
        };
        let mut tasks = current.to_vec();
        tasks.push(Task {
            id: Decimal(next_id),
            label: label.clone(),
            done: false,
        });
        Ok(DomainChange {
            tasks,
            next_id: next_id + 1,
            status: Status::Applied,
        })
    }
}
async fn authentication(pool: &Pool) -> Authentication {
    let (reply, response) = oneshot::channel();
    pool.try_send(Request::Login {
        key: "long-enough-test-key".into(),
        reply,
    })
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}
async fn join(
    pool: &Pool,
    auth: &Authentication,
    room: &str,
) -> Result<(Connected, mpsc::Receiver<ServerMessage>), Error> {
    let (outcomes, receiver) = mpsc::channel(OUTCOMES);
    let (snapshots, _snapshot) = watch::channel(None);
    let (reply, response) = oneshot::channel();
    pool.try_send(Request::Join {
        capability: auth.capability(),
        room: room.into(),
        resume: None,
        outcomes,
        snapshots,
        reply,
    })
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap()
        .map(|connected| (connected, receiver))
}
fn command(pool: &Pool, auth: &Authentication, joined: &Connected, sequence: i64) {
    pool.try_send(Request::Command {
        route: pool.route(&joined.snapshot.room),
        principal: auth.token.clone(),
        connection: joined.connection.clone(),
        message: ClientMessage::Command(Command {
            version: 1,
            incarnation: joined.snapshot.incarnation.clone(),
            namespace: joined.namespace.clone(),
            sequence: Decimal(sequence),
            expected_revision: Decimal(sequence - 1),
            mutation: Mutation::Add {
                label: format!("command-{sequence}"),
            },
        }),
    })
    .unwrap();
}

#[tokio::test]
async fn a_blocked_worker_cannot_stop_other_rooms_or_global_revocation() {
    let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
    config.workers = 2;
    let gate = Arc::new(Gate {
        open: Mutex::new(false),
        changed: Condvar::new(),
    });
    let release = Release(gate.clone());
    let (entered, response) = oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let calls = Arc::new(AtomicUsize::new(0));
    let factory: Factory = {
        let calls = calls.clone();
        let gate = gate.clone();
        Arc::new(move |worker| {
            Ok(Box::new(Application {
                worker,
                thread: std::thread::current().id(),
                local: Rc::new(Cell::new(0)),
                entered: entered.clone(),
                calls: calls.clone(),
                gate: gate.clone(),
            }))
        })
    };
    let pool = start_with_factory(config, factory).unwrap();
    assert_eq!(pool.route("a"), Route(0));
    assert_eq!(pool.route("b"), Route(1));
    let auth = authentication(&pool).await;
    let (blocked, mut blocked_outcomes) = join(&pool, &auth, "a").await.unwrap();
    command(&pool, &auth, &blocked, 1);
    tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap();
    let metrics = observed(&pool, |snapshot| {
        snapshot.workers[1].state == WorkerState::Idle
            && snapshot.authentication.retained_sessions == 1
    })
    .await;
    assert_eq!(metrics.workers.len(), 2);
    assert_eq!(metrics.workers[0].state, WorkerState::Busy);
    assert_eq!(
        (
            metrics.workers[0].rooms,
            metrics.workers[0].connections,
            metrics.workers[0].subscriptions
        ),
        (1, 1, 1)
    );
    assert_eq!(metrics.workers[1].state, WorkerState::Idle);
    assert_eq!(metrics.authentication.retained_sessions, 1);
    assert_eq!(
        (metrics.ingress_in_use, metrics.ingress_limit),
        (1, INGRESS)
    );
    assert_eq!(
        pool.admission.available_permits(),
        INGRESS - 1,
        "an executing domain call must retain its ingress permit"
    );
    let (free, mut free_outcomes) = join(&pool, &auth, "b").await.unwrap();
    command(&pool, &auth, &free, 1);
    let outcome = tokio::time::timeout(Duration::from_secs(5), free_outcomes.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(outcome, ServerMessage::Outcome(ref outcome) if outcome.status == Status::Applied && outcome.revision == Decimal(1))
    );
    assert!(
        !*gate.open.lock().unwrap(),
        "independent room must complete before the blocked worker resumes"
    );
    command(&pool, &auth, &blocked, 2);
    let (reply, response) = oneshot::channel();
    pool.try_send(Request::Logout {
        token: auth.token.clone(),
        csrf: auth.csrf.clone(),
        reply,
    })
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(*auth.revoked.borrow());
    assert!(
        !*gate.open.lock().unwrap(),
        "revocation must not wait for Fern execution"
    );
    let revoked = observed(&pool, |snapshot| {
        snapshot.authentication.retained_sessions == 0
    })
    .await;
    assert_eq!(revoked.workers[0].state, WorkerState::Busy);
    drop(release);
    while tokio::time::timeout(Duration::from_secs(5), blocked_outcomes.recv())
        .await
        .unwrap()
        .is_some()
    {}
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the queued command was revoked before actor admission"
    );
}

#[tokio::test]
async fn worker_count_does_not_multiply_room_or_ingress_admission() {
    let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
    config.workers = 2;
    config.limits.max_rooms = 1;
    let pool = start(config).unwrap();
    let auth = authentication(&pool).await;
    let (_first, _outcomes) = join(&pool, &auth, "a").await.unwrap();
    assert!(matches!(
        join(&pool, &auth, "b").await,
        Err(Error::RoomLimit)
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        while pool.admission.available_permits() != INGRESS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let permits: Vec<_> = (0..INGRESS)
        .map(|_| pool.admission.clone().try_acquire_owned().unwrap())
        .collect();
    let observation = pool.snapshot();
    assert_eq!(observation.ingress_in_use, INGRESS);
    assert_eq!(observation.ingress_limit, INGRESS);
    assert_eq!(
        observation
            .workers
            .iter()
            .map(|worker| worker.rooms)
            .sum::<usize>(),
        1
    );
    let (reply, _response) = oneshot::channel();
    assert!(
        pool.try_send(Request::Authenticate {
            token: auth.token.clone(),
            csrf: None,
            reply
        })
        .is_err()
    );
    let (reply, _response) = oneshot::channel();
    let (outcomes, _rx) = mpsc::channel(OUTCOMES);
    let (snapshots, _rx) = watch::channel(None);
    assert!(
        pool.try_send(Request::Join {
            capability: auth.capability(),
            room: "b".into(),
            resume: None,
            outcomes,
            snapshots,
            reply
        })
        .is_err()
    );
    drop(permits);
    assert_eq!(pool.admission.available_permits(), INGRESS);
}

#[tokio::test]
async fn moving_between_workers_releases_the_old_connection_before_joining() {
    let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
    config.workers = 2;
    config.limits.max_connections = 1;
    let pool = start(config).unwrap();
    let auth = authentication(&pool).await;
    let (old, mut old_outcomes) = join(&pool, &auth, "a").await.unwrap();
    pool.disconnect(pool.route("a"), old.connection)
        .await
        .unwrap();
    let (new, _new_outcomes) = join(&pool, &auth, "b").await.unwrap();
    assert_eq!(new.snapshot.room, "b");
    assert!(old_outcomes.recv().await.is_none());
    let observation = observed(&pool, |snapshot| {
        snapshot
            .workers
            .iter()
            .all(|worker| worker.state == WorkerState::Idle)
            && snapshot.workers[0].connections == 0
            && snapshot.workers[1].connections == 1
    })
    .await;
    assert_eq!(
        (
            observation.workers[0].rooms,
            observation.workers[0].namespaces,
            observation.workers[0].subscriptions
        ),
        (1, 1, 0)
    );
    assert_eq!(
        (
            observation.workers[1].rooms,
            observation.workers[1].namespaces,
            observation.workers[1].subscriptions
        ),
        (1, 1, 1)
    );
    assert_eq!(observation.authentication.retained_sessions, 1);
}

async fn observed(pool: &Pool, predicate: impl Fn(&PoolSnapshot) -> bool) -> PoolSnapshot {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = pool.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owner must publish its completed state")
}

#[tokio::test]
async fn terminal_observations_follow_domain_and_authentication_cleanup() {
    struct DropDomain(Arc<AtomicUsize>);
    impl Drop for DropDomain {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Domain for DropDomain {
        fn apply(
            &mut self,
            _: &str,
            _: &[Task],
            _: i64,
            _: &Mutation,
            _: usize,
        ) -> Result<DomainChange, Error> {
            unreachable!()
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let drops = dropped.clone();
    let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
    config.workers = 2;
    let pool = start_with_factory(
        config,
        Arc::new(move |_| Ok(Box::new(DropDomain(drops.clone())))),
    )
    .unwrap();
    let auth = authentication(&pool).await;
    observed(&pool, |snapshot| {
        snapshot.authentication.retained_sessions == 1
    })
    .await;
    let workers = pool.worker_metrics.clone();
    let authentication = pool.auth_metrics.clone();
    drop(pool);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if workers
                .iter()
                .all(|worker| worker.borrow().state == WorkerState::Stopped)
                && authentication.borrow().stopped
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        2,
        "stopped publication must follow Fern domain destruction"
    );
    assert!(
        *auth.revoked.borrow(),
        "authentication shutdown must revoke retained sessions"
    );
    assert_eq!(authentication.borrow().retained_sessions, 0);
    for (index, worker) in workers.iter().enumerate() {
        assert_eq!(*worker.borrow(), WorkerSnapshot::stopped(index));
    }
}

#[test]
fn worker_configuration_rejects_zero_and_excess_threads() {
    for workers in [0, 33, usize::MAX] {
        let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        config.workers = workers;
        assert!(start(config).is_err());
    }
}

#[tokio::test]
async fn failed_worker_startup_drops_previously_started_domains_on_their_threads() {
    struct LocalDomain {
        dropped: Option<oneshot::Sender<std::thread::ThreadId>>,
        _local: Rc<()>,
    }
    impl Drop for LocalDomain {
        fn drop(&mut self) {
            let _ = self
                .dropped
                .take()
                .unwrap()
                .send(std::thread::current().id());
        }
    }
    impl Domain for LocalDomain {
        fn apply(
            &mut self,
            _: &str,
            _: &[Task],
            _: i64,
            _: &Mutation,
            _: usize,
        ) -> Result<DomainChange, Error> {
            unreachable!()
        }
    }
    let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
    config.workers = 2;
    let (dropped, response) = oneshot::channel();
    let dropped = Mutex::new(Some(dropped));
    let caller = std::thread::current().id();
    let factory: Factory = Arc::new(move |index| {
        if index == 1 {
            return Err(std::io::Error::other("injected startup failure"));
        }
        Ok(Box::new(LocalDomain {
            dropped: dropped.lock().unwrap().take(),
            _local: Rc::new(()),
        }))
    });
    assert!(start_with_factory(config, factory).is_err());
    let owner = tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(owner, caller, "a non-Send domain must drop on its worker");
}
