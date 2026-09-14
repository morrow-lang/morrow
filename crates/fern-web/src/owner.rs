//! Each pinned actor worker owns its local Fern heaps, rooms and subscriptions.
use crate::Config;
use axum::http::StatusCode;
use fern_web_protocol::{ClientMessage, Connected, Error, Hub, ServerMessage, Snapshot};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

pub(crate) const INGRESS: usize = 256;
pub(crate) const OUTCOMES: usize = 16;
type Reply<T> = oneshot::Sender<Result<T, StatusCode>>;

mod auth;
mod pool;
pub(crate) use auth::Authentication;
pub(crate) use auth::Capability;
pub(crate) use pool::{Pool, PoolSnapshot, Route, WorkerState, start};

struct Subscription {
    principal: String,
    room: String,
    namespace: String,
    outcomes: mpsc::Sender<ServerMessage>,
    snapshots: watch::Sender<Option<Snapshot>>,
}
pub(crate) enum Request {
    Login {
        key: String,
        reply: Reply<Authentication>,
    },
    Authenticate {
        token: String,
        csrf: Option<String>,
        reply: Reply<Authentication>,
    },
    Logout {
        token: String,
        csrf: String,
        reply: Reply<()>,
    },
    Join {
        capability: Capability,
        room: String,
        resume: Option<String>,
        outcomes: mpsc::Sender<ServerMessage>,
        snapshots: watch::Sender<Option<Snapshot>>,
        reply: oneshot::Sender<Result<Connected, Error>>,
    },
    Command {
        route: Route,
        principal: String,
        connection: String,
        message: ClientMessage,
    },
    Disconnect {
        route: Route,
        connection: String,
        reply: Option<oneshot::Sender<()>>,
    },
}

pub(crate) fn token() -> std::io::Result<String> {
    use std::fmt::Write;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
    let mut text = String::with_capacity(64);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").expect("String formatting cannot fail");
    }
    Ok(text)
}
// Fixed-size secret comparison avoids returning at the first differing byte.
fn same_secret(a: &str, b: &str) -> bool {
    let mut difference = a.len() ^ b.len();
    for i in 0..256 {
        difference |= usize::from(
            a.as_bytes().get(i).copied().unwrap_or(0) ^ b.as_bytes().get(i).copied().unwrap_or(0),
        );
    }
    difference == 0
}

struct Owner {
    hub: Hub,
    started: Instant,
    capabilities: BTreeMap<String, Capability>,
    subscriptions: BTreeMap<String, Subscription>,
}
impl Owner {
    fn now(&self) -> u64 {
        self.started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
    fn expire(&mut self) {
        let expired: Vec<_> = self
            .capabilities
            .values()
            .filter(|capability| !capability.valid())
            .map(|capability| capability.principal.clone())
            .collect();
        for principal in expired {
            self.revoke(&principal);
        }
        let now_ms = self.now();
        let _ = self.hub.expire(now_ms);
        self.capabilities.retain(|namespace, capability| {
            capability.valid() && self.hub.namespace_is_live(namespace, now_ms)
        });
        let disconnected: Vec<_> = self
            .subscriptions
            .iter()
            .filter(|(id, s)| s.outcomes.is_closed() || !self.hub.connection_is_live(id, now_ms))
            .map(|(id, _)| id.clone())
            .collect();
        for id in disconnected {
            self.disconnect(&id);
        }
    }
    fn disconnect(&mut self, id: &str) {
        self.subscriptions.remove(id);
        self.hub.disconnect(id);
    }
    fn revoke(&mut self, token: &str) {
        self.hub.revoke(token);
        self.capabilities
            .retain(|_, capability| capability.principal != token);
        self.subscriptions
            .retain(|_, subscription| subscription.principal != token);
    }
    fn handle(&mut self, request: Request) {
        self.expire();
        match request {
            Request::Join {
                capability,
                room,
                resume,
                outcomes,
                snapshots,
                reply,
            } => {
                let principal = capability.principal.clone();
                if !capability.valid() {
                    let _ = reply.send(Err(Error::Unauthorized));
                    return;
                }
                let result = self
                    .hub
                    .connect(&principal, &room, resume.as_deref(), self.now());
                if let Ok(connected) = &result {
                    self.capabilities
                        .insert(connected.namespace.clone(), capability);
                    // A resumed namespace takes over its old physical connection.
                    self.subscriptions
                        .retain(|_, s| s.namespace != connected.namespace);
                    self.subscriptions.insert(
                        connected.connection.clone(),
                        Subscription {
                            principal,
                            room,
                            namespace: connected.namespace.clone(),
                            outcomes,
                            snapshots,
                        },
                    );
                }
                if let Err(Ok(connected)) = reply.send(result) {
                    self.disconnect(&connected.connection);
                }
            }
            Request::Command {
                route: _,
                principal,
                connection,
                message,
            } => {
                let ClientMessage::Command(command) = message else {
                    return;
                };
                let Some(subscription) = self.subscriptions.get(&connection) else {
                    return;
                };
                if subscription.principal != principal
                    || self
                        .capabilities
                        .get(&subscription.namespace)
                        .is_none_or(|capability| !capability.valid())
                {
                    self.disconnect(&connection);
                    return;
                }
                let room = subscription.room.clone();
                let before = self.hub.snapshot(&room).ok().map(|s| s.incarnation);
                let result = self
                    .hub
                    .command(&principal, &connection, command, self.now());
                let changed = result
                    .as_ref()
                    .is_ok_and(|outcome| outcome.status == fern_web_protocol::Status::Applied);
                let has_outcome = result.is_ok();
                let response = match result {
                    Ok(outcome) => ServerMessage::Outcome(outcome),
                    Err(error) => ServerMessage::Error(error),
                };
                // Outcomes cannot be coalesced. A full channel retires the socket;
                // reconnect can retrieve the bounded dedupe outcome from the Hub.
                if self.subscriptions[&connection]
                    .outcomes
                    .try_send(response)
                    .is_err()
                {
                    self.disconnect(&connection);
                }
                let snapshot = self.hub.snapshot(&room);
                if snapshot
                    .as_ref()
                    .is_ok_and(|s| Some(&s.incarnation) != before.as_ref())
                {
                    if let Ok(snapshot) = snapshot {
                        for subscription in self.subscriptions.values().filter(|s| s.room == room) {
                            subscription.snapshots.send_replace(Some(snapshot.clone()));
                        }
                    }
                    return;
                }
                if snapshot.is_err() {
                    let ids: Vec<_> = self
                        .subscriptions
                        .iter()
                        .filter(|(_, s)| s.room == room)
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in ids {
                        self.disconnect(&id);
                    }
                    return;
                }
                if changed && let Ok(snapshot) = self.hub.snapshot(&room) {
                    for subscription in self.subscriptions.values().filter(|s| s.room == room) {
                        subscription.snapshots.send_replace(Some(snapshot.clone()));
                    }
                } else if has_outcome
                    && let Ok(snapshot) = self.hub.snapshot(&room)
                    && let Some(subscription) = self.subscriptions.get(&connection)
                {
                    // Conflicts and evicted dedupe outcomes also require current
                    // state before the browser can safely issue its next command.
                    subscription.snapshots.send_replace(Some(snapshot));
                }
            }
            Request::Disconnect {
                route: _,
                connection,
                reply,
            } => {
                self.disconnect(&connection);
                if let Some(reply) = reply {
                    let _ = reply.send(());
                }
            }
            _ => unreachable!("dispatcher sends only worker requests"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn resumed_subscription_closes_at_original_namespace_expiry() {
        let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        config.limits.namespace_ttl_ms = 1000;
        config.limits.outcome_ttl_ms = 500;
        let mut authentication = auth::AuthenticationOwner::new(config.clone());
        let mut owner = Owner {
            hub: Hub::new("boot".into(), config.limits.clone()).unwrap(),
            started: Instant::now(),
            capabilities: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        let auth = authentication.login("long-enough-test-key").unwrap();
        let first = owner.hub.connect(&auth.token, "room", None, 0).unwrap();
        owner.hub.disconnect(&first.connection);
        tokio::time::advance(Duration::from_millis(900)).await;
        let (outcomes, mut outcome_rx) = mpsc::channel(OUTCOMES);
        let (snapshots, snapshot_rx) = watch::channel(None);
        let (reply, response) = oneshot::channel();
        owner.handle(Request::Join {
            capability: auth.capability(),
            room: "room".into(),
            resume: Some(first.namespace),
            outcomes,
            snapshots,
            reply,
        });
        let resumed = response.await.unwrap().unwrap();
        assert!(resumed.resumed);
        tokio::time::advance(Duration::from_millis(99)).await;
        owner.expire();
        assert_eq!(owner.hub.counts(), (1, 1, 1));
        assert_eq!(owner.subscriptions.len(), 1);
        tokio::time::advance(Duration::from_millis(1)).await;
        owner.expire();
        assert_eq!(owner.hub.counts(), (1, 0, 0));
        assert!(
            owner.subscriptions.is_empty(),
            "expired namespace retained transport subscriptions"
        );
        assert!(outcome_rx.is_closed());
        assert!(outcome_rx.try_recv().is_err());
        assert!(snapshot_rx.has_changed().is_err());
        assert!(
            authentication
                .authentication(&auth.token, Some(&auth.csrf))
                .is_ok(),
            "namespace expiry must not revoke unrelated authentication"
        );
    }
    #[tokio::test(start_paused = true)]
    async fn expired_session_revokes_watchers_and_command_namespaces() {
        let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        config.session_ttl = Duration::from_millis(1);
        let mut authentication = auth::AuthenticationOwner::new(config.clone());
        let mut owner = Owner {
            hub: Hub::new("boot".into(), config.limits.clone()).unwrap(),
            started: Instant::now(),
            capabilities: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        let auth = authentication.login("long-enough-test-key").unwrap();
        let connected = owner.hub.connect(&auth.token, "room", None, 0).unwrap();
        owner
            .capabilities
            .insert(connected.namespace, auth.capability());
        tokio::time::advance(Duration::from_millis(1)).await;
        authentication.expire();
        owner.expire();
        assert!(*auth.revoked.borrow());
        assert_eq!(owner.hub.counts(), (1, 0, 0));
        assert!(matches!(
            authentication.authentication(&auth.token, Some(&auth.csrf)),
            Err(StatusCode::UNAUTHORIZED)
        ));
    }
    #[tokio::test]
    async fn slow_reader_retains_only_latest_snapshot_and_bounded_outcomes() {
        let config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        let mut authentication = auth::AuthenticationOwner::new(config.clone());
        let mut owner = Owner {
            hub: Hub::new("boot".into(), config.limits.clone()).unwrap(),
            started: Instant::now(),
            capabilities: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        let auth = authentication.login("long-enough-test-key").unwrap();
        let (outcomes, mut rx) = mpsc::channel(OUTCOMES);
        let (snapshots, snapshots_rx) = watch::channel(None);
        let (reply, response) = oneshot::channel();
        owner.handle(Request::Join {
            capability: auth.capability(),
            room: "room".into(),
            resume: None,
            outcomes,
            snapshots,
            reply,
        });
        let connected = response.await.unwrap().unwrap();
        for sequence in 1..=OUTCOMES as i64 + 1 {
            owner.handle(Request::Command {
                route: Route(0),
                principal: auth.token.clone(),
                connection: connected.connection.clone(),
                message: ClientMessage::Command(fern_web_protocol::Command {
                    version: 1,
                    incarnation: connected.snapshot.incarnation.clone(),
                    namespace: connected.namespace.clone(),
                    sequence: fern_web_protocol::Decimal(sequence),
                    expected_revision: fern_web_protocol::Decimal(sequence - 1),
                    mutation: fern_web_protocol::Mutation::Add {
                        label: format!("task {sequence}"),
                    },
                }),
            });
        }
        assert_eq!(owner.subscriptions.len(), 0);
        assert_eq!(owner.hub.counts().2, 0);
        assert_eq!(rx.len(), OUTCOMES);
        assert_eq!(
            snapshots_rx.borrow().as_ref().unwrap().revision.0,
            OUTCOMES as i64
        );
        for _ in 0..OUTCOMES {
            assert!(matches!(rx.recv().await, Some(ServerMessage::Outcome(_))));
        }
        assert!(rx.recv().await.is_none());
    }
    #[tokio::test(start_paused = true)]
    async fn cancelled_join_and_lost_disconnect_release_transport_admission() {
        let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        config.limits.namespace_ttl_ms = 100;
        config.limits.outcome_ttl_ms = 50;
        let budget = fern_web_protocol::Budget::new(&config.limits).unwrap();
        let mut authentication = auth::AuthenticationOwner::new(config.clone());
        let auth = authentication.login("long-enough-test-key").unwrap();
        let mut owner = Owner {
            hub: Hub::with_budget("boot".into(), config.limits, budget.clone()).unwrap(),
            started: Instant::now(),
            capabilities: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        for cancelled in [true, false] {
            let (outcomes, receiver) = mpsc::channel(OUTCOMES);
            let (snapshots, _snapshots) = watch::channel(None);
            let (reply, response) = oneshot::channel();
            let response = if cancelled {
                drop(response);
                None
            } else {
                Some(response)
            };
            owner.handle(Request::Join {
                capability: auth.capability(),
                room: "room".into(),
                resume: None,
                outcomes,
                snapshots,
                reply,
            });
            if let Some(response) = response {
                assert!(response.await.unwrap().is_ok());
                assert_eq!(budget.used(), (1, 1, 1));
            }
            // The socket can lose its best-effort Disconnect during overload.
            drop(receiver);
            owner.expire();
            assert_eq!(budget.used(), (1, 1, 0));
            assert!(owner.subscriptions.is_empty());
            tokio::time::advance(Duration::from_millis(100)).await;
            owner.expire();
            assert_eq!(budget.used(), (1, 0, 0));
            assert!(owner.capabilities.is_empty());
        }
        drop(owner);
        assert_eq!(budget.used(), (0, 0, 0));
    }
}
