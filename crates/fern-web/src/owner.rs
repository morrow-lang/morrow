//! One bounded async owner of authentication, rooms and subscription publication.
use crate::Config;
use axum::http::StatusCode;
use fern_web_protocol::{ClientMessage, Connected, Error, Hub, ServerMessage, Snapshot};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

pub(crate) const INGRESS: usize = 256;
pub(crate) const OUTCOMES: usize = 16;
type Reply<T> = oneshot::Sender<Result<T, StatusCode>>;

#[derive(Clone)]
pub(crate) struct Authentication {
    pub token: String,
    pub csrf: String,
    pub revoked: watch::Receiver<bool>,
}
struct Session {
    csrf: String,
    expires: Instant,
    revoke: watch::Sender<bool>,
}
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
        principal: String,
        room: String,
        resume: Option<String>,
        previous: Option<String>,
        outcomes: mpsc::Sender<ServerMessage>,
        snapshots: watch::Sender<Option<Snapshot>>,
        reply: oneshot::Sender<Result<Connected, Error>>,
    },
    Command {
        principal: String,
        connection: String,
        message: ClientMessage,
    },
    Disconnect(String),
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
    config: Config,
    hub: Hub,
    started: Instant,
    sessions: BTreeMap<String, Session>,
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
        let now = Instant::now();
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.expires <= now)
            .map(|(token, _)| token.clone())
            .collect();
        for token in expired {
            self.revoke(&token);
        }
        let now_ms = self.now();
        let _ = self.hub.expire(now_ms);
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
        if let Some(session) = self.sessions.remove(token) {
            session.revoke.send_replace(true);
        }
        self.hub.revoke(token);
        self.subscriptions.retain(|_, s| s.principal != token);
    }
    fn authentication(
        &self,
        token: &str,
        csrf: Option<&str>,
    ) -> Result<Authentication, StatusCode> {
        let session = self.sessions.get(token).ok_or(StatusCode::UNAUTHORIZED)?;
        if csrf.is_some_and(|csrf| !same_secret(csrf, &session.csrf)) {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(Authentication {
            token: token.into(),
            csrf: session.csrf.clone(),
            revoked: session.revoke.subscribe(),
        })
    }
    fn login(&mut self, key: &str) -> Result<Authentication, StatusCode> {
        if !same_secret(key, &self.config.access_key) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if self.sessions.len() >= self.config.max_sessions {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let token = token().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let csrf = self::token().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let (revoke, revoked) = watch::channel(false);
        self.sessions.insert(
            token.clone(),
            Session {
                csrf: csrf.clone(),
                expires: Instant::now() + self.config.session_ttl,
                revoke,
            },
        );
        Ok(Authentication {
            token,
            csrf,
            revoked,
        })
    }
    fn handle(&mut self, request: Request) {
        self.expire();
        match request {
            Request::Login { key, reply } => {
                let _ = reply.send(self.login(&key));
            }
            Request::Authenticate { token, csrf, reply } => {
                let _ = reply.send(self.authentication(&token, csrf.as_deref()));
            }
            Request::Logout { token, csrf, reply } => {
                let result = self
                    .authentication(&token, Some(&csrf))
                    .map(|_| self.revoke(&token));
                let _ = reply.send(result);
            }
            Request::Join {
                principal,
                room,
                resume,
                previous,
                outcomes,
                snapshots,
                reply,
            } => {
                if !self.sessions.contains_key(&principal) {
                    let _ = reply.send(Err(Error::Unauthorized));
                    return;
                }
                if let Some(previous) = previous {
                    self.disconnect(&previous);
                }
                let result = self
                    .hub
                    .connect(&principal, &room, resume.as_deref(), self.now());
                if let Ok(connected) = &result {
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
                if subscription.principal != principal || !self.sessions.contains_key(&principal) {
                    self.disconnect(&connection);
                    return;
                }
                let room = subscription.room.clone();
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
            Request::Disconnect(connection) => self.disconnect(&connection),
        }
    }
}

pub(crate) fn start(config: Config) -> std::io::Result<mpsc::Sender<Request>> {
    let hub = Hub::new(token()?, config.limits.clone()).map_err(std::io::Error::other)?;
    let (tx, mut rx) = mpsc::channel(INGRESS);
    let mut owner = Owner {
        config,
        hub,
        started: Instant::now(),
        sessions: BTreeMap::new(),
        subscriptions: BTreeMap::new(),
    };
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                request = rx.recv() => match request { Some(request) => owner.handle(request), None => break },
                _ = timer.tick() => owner.expire(),
            }
        }
    });
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn resumed_subscription_closes_at_original_namespace_expiry() {
        let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        config.limits.namespace_ttl_ms = 1000;
        config.limits.outcome_ttl_ms = 500;
        let mut owner = Owner {
            hub: Hub::new("boot".into(), config.limits.clone()).unwrap(),
            config,
            started: Instant::now(),
            sessions: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        let auth = owner.login("long-enough-test-key").unwrap();
        let first = owner.hub.connect(&auth.token, "room", None, 0).unwrap();
        owner.hub.disconnect(&first.connection);
        tokio::time::advance(Duration::from_millis(900)).await;
        let (outcomes, mut outcome_rx) = mpsc::channel(OUTCOMES);
        let (snapshots, snapshot_rx) = watch::channel(None);
        let (reply, response) = oneshot::channel();
        owner.handle(Request::Join {
            principal: auth.token.clone(),
            room: "room".into(),
            resume: Some(first.namespace),
            previous: None,
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
            owner.authentication(&auth.token, Some(&auth.csrf)).is_ok(),
            "namespace expiry must not revoke unrelated authentication"
        );
    }
    #[tokio::test]
    async fn expired_session_revokes_watchers_and_command_namespaces() {
        let config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        let mut owner = Owner {
            hub: Hub::new("boot".into(), config.limits.clone()).unwrap(),
            config,
            started: Instant::now(),
            sessions: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        let auth = owner.login("long-enough-test-key").unwrap();
        owner.hub.connect(&auth.token, "room", None, 0).unwrap();
        owner.sessions.get_mut(&auth.token).unwrap().expires = Instant::now();
        owner.expire();
        assert!(*auth.revoked.borrow());
        assert_eq!(owner.hub.counts(), (1, 0, 0));
        assert!(matches!(
            owner.authentication(&auth.token, Some(&auth.csrf)),
            Err(StatusCode::UNAUTHORIZED)
        ));
    }
    #[tokio::test]
    async fn slow_reader_retains_only_latest_snapshot_and_bounded_outcomes() {
        let config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        let mut owner = Owner {
            hub: Hub::new("boot".into(), config.limits.clone()).unwrap(),
            config,
            started: Instant::now(),
            sessions: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        };
        let auth = owner.login("long-enough-test-key").unwrap();
        let (outcomes, mut rx) = mpsc::channel(OUTCOMES);
        let (snapshots, snapshots_rx) = watch::channel(None);
        let (reply, response) = oneshot::channel();
        owner.handle(Request::Join {
            principal: auth.token.clone(),
            room: "room".into(),
            resume: None,
            previous: None,
            outcomes,
            snapshots,
            reply,
        });
        let connected = response.await.unwrap().unwrap();
        for sequence in 1..=OUTCOMES as i64 + 1 {
            owner.handle(Request::Command {
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
}
