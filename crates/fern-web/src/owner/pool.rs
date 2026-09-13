//! Stable room placement across pinned workers with one global ingress budget.
use super::*;
use fern_web_protocol::{Budget, Domain};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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
}
impl Pool {
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
            Request::Login { .. } | Request::Authenticate { .. } | Request::Logout { .. } => {
                &self.auth
            }
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
        .map(fern_web_app::SharedCheckpoint::open)
        .transpose()?;
    start_with_factory(
        config,
        Arc::new(move |_| {
            let domain = checkpoint
                .as_ref()
                .map_or_else(fern_web_app::NativeDomain::new, |checkpoint| {
                    fern_web_app::NativeDomain::with_checkpoint(checkpoint.clone())
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
    for index in 0..config.workers {
        let (tx, mut rx) = mpsc::channel::<Envelope>(INGRESS);
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let factory = factory.clone();
        let limits = config.limits.clone();
        let budget = budget.clone();
        let incarnation = token()?;
        std::thread::Builder::new().name(format!("fern-actors-{index}")).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_time().build() {
                Ok(runtime) => runtime,
                Err(error) => { let _ = ready.send(Err(error)); return; }
            };
            runtime.block_on(async move {
                // Both construction and destruction happen on this thread. The
                // factory exchanges only Rust checkpoint handles, never Fern heaps.
                let domain = match factory(index) { Ok(domain) => domain, Err(error) => { let _ = ready.send(Err(error)); return; } };
                let hub = match Hub::with_domain_and_budget(incarnation, limits, domain, budget) {
                    Ok(hub) => hub,
                    Err(error) => { let _ = ready.send(Err(std::io::Error::other(error))); return; }
                };
                let mut owner = Owner { hub, started: Instant::now(), capabilities: BTreeMap::new(), subscriptions: BTreeMap::new() };
                if ready.send(Ok(())).is_err() { return; }
                let mut timer = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tokio::select! {
                        envelope = rx.recv() => match envelope { Some(envelope) => owner.handle(envelope.request), None => break },
                        _ = timer.tick() => owner.expire(),
                    }
                }
            });
        })?;
        started.recv().map_err(std::io::Error::other)??;
        workers.push(tx);
    }
    let (auth, mut requests) = mpsc::channel::<Envelope>(INGRESS);
    tokio::spawn(async move {
        let mut owner = auth::AuthenticationOwner::new(config);
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                envelope = requests.recv() => match envelope { Some(envelope) => owner.handle(envelope.request), None => break },
                _ = timer.tick() => owner.expire(),
            }
        }
    });
    Ok(Pool {
        auth,
        workers: Arc::new(workers),
        admission,
    })
}
