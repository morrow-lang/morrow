//! Stable room placement across pinned workers with one global ingress budget.
use super::*;
use morrow_web_protocol::{Budget, Domain};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

mod telemetry;
pub(crate) use telemetry::{AuthenticationSnapshot, PoolSnapshot, WorkerSnapshot, WorkerState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Route(pub(super) usize);
struct Envelope {
    request: Request,
    _permit: OwnedSemaphorePermit,
}
#[derive(Clone)]
pub(crate) struct Pool {
    auth: mpsc::Sender<Envelope>,
    workers: Arc<Vec<mpsc::Sender<Envelope>>>,
    admission: Arc<Semaphore>,
    worker_metrics: Arc<Vec<watch::Receiver<WorkerSnapshot>>>,
    auth_metrics: watch::Receiver<AuthenticationSnapshot>,
}
impl Pool {
    /// Copy at most 32 short observations without entering the worker queues.
    /// Counts describe each owner's last published turn, not a globally atomic
    /// instant. No telemetry borrow is held while Morrow callbacks or IO execute.
    pub fn snapshot(&self) -> PoolSnapshot {
        let workers = self
            .worker_metrics
            .iter()
            .enumerate()
            .map(|(index, receiver)| {
                if receiver.has_changed().is_err() {
                    WorkerSnapshot::stopped(index)
                } else {
                    *receiver.borrow()
                }
            })
            .collect();
        let authentication = if self.auth_metrics.has_changed().is_err() {
            AuthenticationSnapshot {
                stopped: true,
                retained_sessions: 0,
            }
        } else {
            *self.auth_metrics.borrow()
        };
        PoolSnapshot {
            workers,
            authentication,
            ingress_in_use: INGRESS - self.admission.available_permits(),
            ingress_limit: INGRESS,
        }
    }
    pub fn route(&self, room: &str) -> Route {
        let hash = room.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        Route((hash % self.workers.len() as u64) as usize)
    }
    pub async fn disconnect(&self, route: Route, connection: String) -> Result<(), ()> {
        let (reply, response) = oneshot::channel();
        self.try_send(Request::Disconnect {
            route,
            connection,
            reply: Some(reply),
        })?;
        response.await.map_err(|_| ())
    }
    pub fn try_send(&self, request: Request) -> Result<(), ()> {
        let sender = match &request {
            Request::Open { .. }
            | Request::Login { .. }
            | Request::Authenticate { .. }
            | Request::Logout { .. } => &self.auth,
            Request::Join { room, .. } => &self.workers[self.route(room).0],
            Request::Command { route, .. } | Request::Disconnect { route, .. } => {
                self.workers.get(route.0).ok_or(())?
            }
        };
        let permit = self.admission.clone().try_acquire_owned().map_err(|_| ())?;
        sender
            .try_send(Envelope {
                request,
                _permit: permit,
            })
            .map_err(|_| ())
    }
}
type Factory = Arc<dyn Fn(usize) -> std::io::Result<Box<dyn Domain>> + Send + Sync>;
#[cfg(test)]
mod tests;

pub(crate) fn start(config: Config) -> std::io::Result<Pool> {
    let checkpoint = config
        .data_dir
        .as_deref()
        .map(|directory| match &config.cluster {
            Some(settings) => morrow_web_app::SharedCheckpoint::open_scoped(
                directory,
                &crate::peer::placement(settings),
            ),
            None => morrow_web_app::SharedCheckpoint::open(directory),
        })
        .transpose()?;
    start_with_factory(
        config,
        Arc::new(move |_| {
            let domain = checkpoint
                .as_ref()
                .map_or_else(morrow_web_app::NativeDomain::new, |checkpoint| {
                    morrow_web_app::NativeDomain::with_checkpoint(checkpoint.clone())
                });
            Ok(Box::new(domain))
        }),
    )
}
fn start_with_factory(config: Config, factory: Factory) -> std::io::Result<Pool> {
    if !(1..=32).contains(&config.workers) {
        return Err(std::io::Error::other("worker count must be 1 through 32"));
    }
    let budget = Budget::new(&config.limits).map_err(std::io::Error::other)?;
    let admission = Arc::new(Semaphore::new(INGRESS));
    let mut workers = Vec::with_capacity(config.workers);
    let mut worker_metrics = Vec::with_capacity(config.workers);
    for index in 0..config.workers {
        let (tx, mut rx) = mpsc::channel::<Envelope>(INGRESS);
        let (metrics, observation) = watch::channel(WorkerSnapshot::stopped(index));
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let factory = factory.clone();
        let limits = config.limits.clone();
        let budget = budget.clone();
        let incarnation = token()?;
        std::thread::Builder::new()
            .name(format!("morrow-actors-{index}"))
            .spawn(move || {
                // Declared outside the runtime/owner scope: terminal publication
                // follows destruction of the thread-local domain and its Morrow heaps.
                let reporter = telemetry::WorkerReporter {
                    sender: metrics,
                    index,
                };
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let report = &reporter;
                runtime.block_on(async move {
                    // Both construction and destruction happen on this thread. The
                    // factory exchanges only Rust checkpoint handles, never Morrow heaps.
                    let domain = match factory(index) {
                        Ok(domain) => domain,
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return;
                        }
                    };
                    let hub = match Hub::with_domain_and_budget(incarnation, limits, domain, budget)
                    {
                        Ok(hub) => hub,
                        Err(error) => {
                            let _ = ready.send(Err(std::io::Error::other(error)));
                            return;
                        }
                    };
                    let mut owner = Owner {
                        hub,
                        started: Instant::now(),
                        capabilities: BTreeMap::new(),
                        subscriptions: BTreeMap::new(),
                    };
                    report.publish(&owner, WorkerState::Idle);
                    if ready.send(Ok(())).is_err() {
                        return;
                    }
                    let mut timer = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        tokio::select! {
                            envelope = rx.recv() => match envelope { Some(envelope) => {
                                report.publish(&owner, WorkerState::Busy);
                                owner.handle(envelope.request);
                                report.publish(&owner, WorkerState::Idle);
                            }, None => break },
                            _ = timer.tick() => {
                                report.publish(&owner, WorkerState::Busy);
                                owner.expire();
                                report.publish(&owner, WorkerState::Idle);
                            },
                        }
                    }
                });
            })?;
        started.recv().map_err(std::io::Error::other)??;
        workers.push(tx);
        worker_metrics.push(observation);
    }
    let (auth, mut requests) = mpsc::channel::<Envelope>(INGRESS);
    let (metrics, auth_metrics) = watch::channel(AuthenticationSnapshot {
        stopped: false,
        retained_sessions: 0,
    });
    tokio::spawn(async move {
        let report = telemetry::AuthenticationReporter(metrics);
        let mut owner = auth::AuthenticationOwner::new(config);
        report.publish(&owner);
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                envelope = requests.recv() => match envelope { Some(envelope) => {
                    owner.handle(envelope.request);
                    report.publish(&owner);
                }, None => break },
                _ = timer.tick() => { owner.expire(); report.publish(&owner); },
            }
        }
    });
    Ok(Pool {
        auth,
        workers: Arc::new(workers),
        admission,
        worker_metrics: Arc::new(worker_metrics),
        auth_metrics,
    })
}
