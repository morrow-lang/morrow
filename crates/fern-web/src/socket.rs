//! One bounded socket task; snapshot coalescing never overwrites command outcomes.
use crate::{
    App,
    owner::{self, Authentication, Request},
};
use axum::extract::ws::{Message, WebSocket};
use fern_web_protocol::{ClientMessage, Error, ServerMessage, decode, encode};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, mpsc, oneshot, watch};

enum Connection {
    Local { route: owner::Route, id: String },
    Remote(crate::peer::Remote),
}
impl Connection {
    fn send(&self, app: &App, principal: &str, message: ClientMessage) -> Result<(), ()> {
        match self {
            Self::Local { route, id } => app.requests.try_send(Request::Command {
                route: *route,
                principal: principal.into(),
                connection: id.clone(),
                message,
            }),
            Self::Remote(remote) => remote.send(message),
        }
    }
    async fn disconnect(self, app: &App) -> Result<(), ()> {
        match self {
            Self::Local { route, id } => app.requests.disconnect(route, id).await,
            Self::Remote(remote) => {
                remote.disconnect().await;
                Ok(())
            }
        }
    }
    fn retire(self, app: &App) {
        if let Self::Local { route, id } = self {
            let _ = app.requests.try_send(Request::Disconnect {
                route,
                connection: id,
                reply: None,
            });
        }
    }
}

async fn join(
    app: &App,
    auth: &Authentication,
    room: String,
    resume: Option<String>,
    outcomes: mpsc::Sender<ServerMessage>,
    snapshots: watch::Sender<Option<fern_web_protocol::Snapshot>>,
) -> Result<(Connection, fern_web_protocol::Connected), Error> {
    if let Some(cluster) = &app.cluster
        && let Some(target) = cluster.remote_owner(&room)?
    {
        let (remote, connected) = cluster
            .join(&target, auth.clone(), room, outcomes, snapshots)
            .await?;
        return Ok((Connection::Remote(remote), connected));
    }
    let route = app.requests.route(&room);
    let (reply, response) = oneshot::channel();
    app.requests
        .try_send(Request::Join {
            capability: auth.capability(),
            room,
            resume,
            outcomes,
            snapshots,
            reply,
        })
        .map_err(|_| Error::ConnectionLimit)?;
    let connected = response.await.map_err(|_| Error::Offline)??;
    Ok((
        Connection::Local {
            route,
            id: connected.connection.clone(),
        },
        connected,
    ))
}

async fn write(socket: &mut WebSocket, message: Message, limit: Duration) -> bool {
    matches!(
        tokio::time::timeout(limit, socket.send(message)).await,
        Ok(Ok(()))
    )
}
async fn publish(socket: &mut WebSocket, message: ServerMessage, limit: Duration) -> bool {
    let Ok(bytes) = encode(&message) else {
        return false;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return false;
    };
    write(socket, Message::Text(text.into()), limit).await
}

/// Coalescing may skip the exact rollover snapshot. Compare resource identity,
/// rather than revision, so every browser observes the reset before later state.
fn publication(incarnation: &mut Option<String>, message: ServerMessage) -> ServerMessage {
    match message {
        ServerMessage::Connected(connected) => {
            *incarnation = Some(connected.snapshot.incarnation.clone());
            ServerMessage::Connected(connected)
        }
        ServerMessage::Reset(snapshot) => {
            *incarnation = Some(snapshot.incarnation.clone());
            ServerMessage::Reset(snapshot)
        }
        ServerMessage::Snapshot(snapshot) => {
            let changed = incarnation
                .as_ref()
                .is_some_and(|old| old != &snapshot.incarnation);
            *incarnation = Some(snapshot.incarnation.clone());
            if changed {
                ServerMessage::Reset(snapshot)
            } else {
                ServerMessage::Snapshot(snapshot)
            }
        }
        other => other,
    }
}
pub(crate) async fn run(
    mut socket: WebSocket,
    app: App,
    mut auth: Authentication,
    _permit: OwnedSemaphorePermit,
) {
    let mut connection: Option<Connection> = None;
    let mut incarnation = None;
    let (_, mut outcomes) = mpsc::channel(owner::OUTCOMES);
    let (_, mut snapshots) = watch::channel(None);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    let join_deadline = tokio::time::Instant::now() + app.config.handshake_timeout;
    let join_timeout = tokio::time::sleep_until(join_deadline);
    tokio::pin!(join_timeout);
    let mut active = Instant::now();
    let limit = app.config.write_timeout;
    loop {
        if *auth.revoked.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = auth.revoked.changed() => break,
            // Control frames and failed joins cannot renew admission indefinitely.
            _ = &mut join_timeout, if connection.is_none() => break,
            message = outcomes.recv(), if connection.is_some() => {
                let Some(message) = message else { break; };
                if !publish(&mut socket, publication(&mut incarnation, message), limit).await { break; }
            }
            changed = snapshots.changed(), if connection.is_some() => {
                if changed.is_err() { break; }
                let snapshot = snapshots.borrow_and_update().clone();
                if let Some(snapshot) = snapshot
                    && !publish(&mut socket, publication(&mut incarnation, ServerMessage::Snapshot(snapshot)), limit).await { break; }
            }
            incoming = socket.recv() => {
                let Some(Ok(incoming)) = incoming else { break; };
                active = Instant::now();
                match incoming {
                    Message::Text(text) => match decode::<ClientMessage>(text.as_bytes()) {
                        Ok(ClientMessage::Join { room, resume_namespace }) => {
                            let response_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
                            let response_deadline = if connection.is_none() {
                                response_deadline.min(join_deadline)
                            } else { response_deadline };
                            if let Some(old_connection) = connection.take()
                                && !matches!(tokio::time::timeout_at(response_deadline, old_connection.disconnect(&app)).await, Ok(Ok(()))) { break; }
                            let (outcome_tx, outcome_rx) = mpsc::channel(owner::OUTCOMES);
                            let (snapshot_tx, snapshot_rx) = watch::channel(None);
                            let joining = join(&app, &auth, room, resume_namespace, outcome_tx, snapshot_tx);
                            match tokio::time::timeout_at(response_deadline, joining).await {
                                Ok(Ok((joined, connected))) => {
                                    connection = Some(joined);
                                    outcomes = outcome_rx;
                                    snapshots = snapshot_rx;
                                    if !publish(&mut socket, publication(&mut incarnation, ServerMessage::Connected(connected)), limit).await { break; }
                                }
                                Ok(Err(error)) => { if !publish(&mut socket, ServerMessage::Error(error), limit).await { break; } }
                                _ => break,
                            }
                        }
                        Ok(message @ ClientMessage::Command(_)) => {
                            let Some(connection) = &connection else {
                                publish(&mut socket, ServerMessage::Error(Error::Unauthorized), limit).await;
                                break;
                            };
                            if connection.send(&app, &auth.token, message).is_err() { break; }
                        }
                        Err(error) => { publish(&mut socket, ServerMessage::Error(error), limit).await; break; }
                    },
                    Message::Ping(bytes) => { if !write(&mut socket, Message::Pong(bytes), limit).await { break; } }
                    Message::Pong(_) => (),
                    Message::Close(_) => break,
                    Message::Binary(_) => { publish(&mut socket, ServerMessage::Error(Error::Malformed), limit).await; break; }
                }
            }
            _ = heartbeat.tick() => {
                if active.elapsed() > Duration::from_secs(60) { break; }
                if !write(&mut socket, Message::Ping(Vec::new().into()), limit).await { break; }
            }
        }
    }
    if let Some(connection) = connection {
        connection.retire(&app);
    }
    let _ = write(&mut socket, Message::Close(None), limit).await;
}

#[cfg(test)]
mod tests {
    use super::publication;
    use fern_web_protocol::{
        Client, Connected, Decimal, Mutation, ServerMessage, Snapshot, VERSION,
    };

    fn snapshot(incarnation: &str, revision: i64) -> Snapshot {
        Snapshot {
            version: VERSION,
            room: "garden".into(),
            incarnation: incarnation.into(),
            revision: Decimal(revision),
            tasks: vec![],
        }
    }
    fn connected(incarnation: &str) -> Connected {
        Connected {
            version: VERSION,
            connection: "connection".into(),
            namespace: "namespace".into(),
            resumed: false,
            next_sequence: Decimal(1),
            snapshot: snapshot(incarnation, 0),
        }
    }

    #[test]
    fn coalesced_incarnation_change_resets_browser_and_never_replays_pending_work() {
        let mut incarnation = None;
        let first = connected("before");
        publication(&mut incarnation, ServerMessage::Connected(first.clone()));
        let mut browser = Client::new(first).unwrap();
        browser
            .submit(Mutation::Add {
                label: "uncertain".into(),
            })
            .unwrap();
        let latest = snapshot("after", 3);
        let ServerMessage::Reset(reset) =
            publication(&mut incarnation, ServerMessage::Snapshot(latest.clone()))
        else {
            panic!("coalesced owner rollover must be published as Reset");
        };
        assert_eq!(reset, latest);
        browser.accept_snapshot(reset, true).unwrap();
        assert!(browser.uncertain());
        assert!(browser.pending().is_none());
        assert_eq!(browser.snapshot(), &latest);
        assert_eq!(
            publication(
                &mut incarnation,
                ServerMessage::Snapshot(snapshot("after", 4))
            ),
            ServerMessage::Snapshot(snapshot("after", 4))
        );
    }

    #[test]
    fn explicit_reset_and_new_join_replace_the_publication_incarnation() {
        let mut incarnation = None;
        publication(
            &mut incarnation,
            ServerMessage::Connected(connected("first")),
        );
        let reset = ServerMessage::Reset(snapshot("second", 0));
        assert_eq!(publication(&mut incarnation, reset.clone()), reset);
        assert_eq!(incarnation.as_deref(), Some("second"));
        let update = ServerMessage::Snapshot(snapshot("second", 1));
        assert_eq!(publication(&mut incarnation, update.clone()), update);
        let join = ServerMessage::Connected(connected("third"));
        assert_eq!(publication(&mut incarnation, join.clone()), join);
        assert_eq!(incarnation.as_deref(), Some("third"));
        let update = ServerMessage::Snapshot(snapshot("third", 1));
        assert_eq!(publication(&mut incarnation, update.clone()), update);
    }
}
