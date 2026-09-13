//! One bounded socket task; snapshot coalescing never overwrites command outcomes.
use crate::{
    App,
    owner::{self, Authentication, Request},
};
use axum::extract::ws::{Message, WebSocket};
use fern_web_protocol::{ClientMessage, Error, ServerMessage, decode, encode};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, mpsc, oneshot, watch};

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
pub(crate) async fn run(
    mut socket: WebSocket,
    app: App,
    mut auth: Authentication,
    _permit: OwnedSemaphorePermit,
) {
    let mut connection = None;
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
                if !publish(&mut socket, message, limit).await { break; }
            }
            changed = snapshots.changed(), if connection.is_some() => {
                if changed.is_err() { break; }
                let snapshot = snapshots.borrow_and_update().clone();
                if let Some(snapshot) = snapshot
                    && !publish(&mut socket, ServerMessage::Snapshot(snapshot), limit).await { break; }
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
                            if let Some((route, old_connection)) = connection.take()
                                && !matches!(tokio::time::timeout_at(response_deadline, app.requests.disconnect(route, old_connection)).await, Ok(Ok(()))) { break; }
                            let (outcome_tx, outcome_rx) = mpsc::channel(owner::OUTCOMES);
                            let (snapshot_tx, snapshot_rx) = watch::channel(None);
                            let (reply, rx) = oneshot::channel();
                            let route = app.requests.route(&room);
                            let request = Request::Join { capability: auth.capability(), room, resume: resume_namespace,
                                outcomes: outcome_tx, snapshots: snapshot_tx, reply };
                            if app.requests.try_send(request).is_err() { break; }
                            match tokio::time::timeout_at(response_deadline, rx).await {
                                Ok(Ok(Ok(connected))) => {
                                    connection = Some((route, connected.connection.clone()));
                                    outcomes = outcome_rx;
                                    snapshots = snapshot_rx;
                                    if !publish(&mut socket, ServerMessage::Connected(connected), limit).await { break; }
                                }
                                Ok(Ok(Err(error))) => { if !publish(&mut socket, ServerMessage::Error(error), limit).await { break; } }
                                _ => break,
                            }
                        }
                        Ok(message @ ClientMessage::Command(_)) => {
                            let Some((route, connection)) = &connection else {
                                publish(&mut socket, ServerMessage::Error(Error::Unauthorized), limit).await;
                                break;
                            };
                            if app.requests.try_send(Request::Command { route: *route, principal: auth.token.clone(), connection: connection.clone(), message }).is_err() { break; }
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
    if let Some((route, connection)) = connection {
        let _ = app.requests.try_send(Request::Disconnect {
            route,
            connection,
            reply: None,
        });
    }
    let _ = write(&mut socket, Message::Close(None), limit).await;
}
