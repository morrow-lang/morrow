//! IO drives the same bounded lease/request state exercised by deterministic simulations.
use super::*;
use fern_cluster::{Frame, LinkState, PeerStream, RequestId};
use fern_web_protocol::Command;
use std::time::Duration;
use tokio::time::Instant;

fn now(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}

pub(super) struct Gateway {
    pub auth: Authentication,
    pub lease_ms: u32,
    pub commands: mpsc::Receiver<ClientMessage>,
    pub stopped: oneshot::Receiver<()>,
    pub outcomes: mpsc::Sender<ServerMessage>,
    pub snapshots: watch::Sender<Option<Snapshot>>,
    pub room: String,
}

pub(super) async fn gateway(
    cluster: Cluster,
    mut stream: PeerStream,
    mut gateway: Gateway,
) -> io::Result<()> {
    let mut shutdown = cluster.0.shutdown.clone();
    let started = Instant::now();
    let mut state = LinkState::new(
        stream.local.link,
        Default::default(),
        u64::from(gateway.lease_ms),
        0,
    )
    .map_err(io::Error::other)?;
    let mut pending: Option<(RequestId, Command)> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut ping_at = 0;
    let mut ping = None;
    loop {
        if !gateway.auth.capability().valid() {
            break;
        }
        if shutdown.has_changed().is_err()
            || !matches!(
                gateway.stopped.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            )
        {
            break;
        }
        let loss = state.expire(now(started)).map_err(io::Error::other)?;
        if loss.link_lost || !loss.requests.is_empty() {
            break;
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = &mut gateway.stopped => break,
            _ = gateway.auth.revoked.changed() => break,
            _ = gateway.outcomes.closed() => break,
            frame = stream.reader.read() => {
                let Some(frame) = frame? else { break; };
                state.peer_activity(now(started)).map_err(io::Error::other)?;
                match frame {
                    Frame::Event(ServerMessage::Snapshot(snapshot)) if snapshot.room == gateway.room => { gateway.snapshots.send_replace(Some(snapshot)); }
                    Frame::Event(ServerMessage::Reset(snapshot)) if snapshot.room == gateway.room => {
                        gateway.outcomes.try_send(ServerMessage::Reset(snapshot)).map_err(|_| invalid("gateway outcome queue full"))?;
                    }
                    Frame::Event(ServerMessage::Outcome(outcome)) => {
                        let Some((id, command)) = pending.take() else { return Err(invalid("unsolicited peer outcome")); };
                        if outcome.namespace != command.namespace || outcome.sequence != command.sequence || outcome.incarnation != command.incarnation { return Err(invalid("peer outcome identity mismatch")); }
                        state.ack(id, now(started)).map_err(io::Error::other)?;
                        gateway.outcomes.try_send(ServerMessage::Outcome(outcome)).map_err(|_| invalid("gateway outcome queue full"))?;
                    }
                    Frame::Event(ServerMessage::Error(error)) => {
                        if let Some((id, _)) = pending.take() { state.ack(id, now(started)).map_err(io::Error::other)?; }
                        gateway.outcomes.try_send(ServerMessage::Error(error)).map_err(|_| invalid("gateway outcome queue full"))?;
                    }
                    Frame::Ping { nonce } => stream.writer.write(&Frame::Pong { nonce }).await?,
                    Frame::Pong { nonce } if ping == Some(nonce) => { ping = None; }
                    Frame::Close => break,
                    _ => return Err(invalid("unexpected owner frame")),
                }
            }
            command = gateway.commands.recv(), if pending.is_none() => {
                let Some(ClientMessage::Command(command)) = command else { break; };
                let bytes = fern_web_protocol::encode(&command).map_err(io::Error::other)?.len();
                let id = state.admit(bytes, now(started)).map_err(io::Error::other)?;
                pending = Some((id, command.clone()));
                stream.writer.write(&Frame::Command(command)).await?;
                cluster.0.forwarded.fetch_add(1, Ordering::Relaxed);
            }
            _ = tick.tick() => {
                let current = now(started);
                if current >= ping_at {
                    if ping.is_some() { break; }
                    ping = Some(current);
                    ping_at = current.saturating_add(10_000);
                    stream.writer.write(&Frame::Ping { nonce: current }).await?;
                }
            }
        }
    }
    let _ = state.disconnect(now(started));
    // Dropping both TLS halves closes the physical authority. A partial write is
    // never retried and no admitted mutation is carried to another stream.
    Ok(())
}

pub(super) async fn serve(
    cluster: Cluster,
    mut stream: PeerStream,
    requests: Pool,
) -> io::Result<()> {
    let mut shutdown = cluster.0.shutdown.clone();
    let first = tokio::select! {
        _ = shutdown.changed() => return Ok(()),
        first = stream.reader.read() => first?,
    };
    let Some(Frame::Join { room, lease_ms }) = first else {
        return Err(invalid("peer must join first"));
    };
    if cluster
        .remote_owner(&room)
        .map_err(io::Error::other)?
        .is_some()
    {
        return Err(invalid("forwarded room belongs to another node"));
    }
    let started = Instant::now();
    let mut state = LinkState::new(
        stream.local.link,
        Default::default(),
        u64::from(lease_ms),
        0,
    )
    .map_err(io::Error::other)?;
    let principal = owner::token()?;
    let (revoke, revoked) = watch::channel(false);
    let capability = owner::Capability::delegated(principal.clone(), revoked, lease_ms);
    let (outcomes, mut responses) = mpsc::channel(owner::OUTCOMES);
    let (snapshots, mut changes) = watch::channel(None);
    let (reply, response) = oneshot::channel();
    let route = requests.route(&room);
    requests
        .try_send(owner::Request::Join {
            capability,
            room,
            resume: None,
            outcomes,
            snapshots,
            reply,
        })
        .map_err(|_| invalid("owner ingress full"))?;
    let connected = match tokio::time::timeout(Duration::from_secs(2), response).await {
        Ok(Ok(Ok(connected))) => connected,
        Ok(Ok(Err(error))) => {
            stream
                .writer
                .write(&Frame::Event(ServerMessage::Error(error)))
                .await?;
            return Ok(());
        }
        _ => return Err(invalid("owner join unavailable")),
    };
    let connection = connected.connection.clone();
    // This guard revokes queued work even if a write fails, a task is cancelled,
    // or the explicit disconnect cannot enter a full worker queue.
    let authority = Authority {
        revoke,
        requests,
        route,
        connection,
    };
    stream
        .writer
        .write(&Frame::Event(ServerMessage::Connected(connected)))
        .await?;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut ping_at = 0;
    let mut ping = None;
    loop {
        if shutdown.has_changed().is_err() {
            break;
        }
        if state
            .expire(now(started))
            .map_err(io::Error::other)?
            .link_lost
        {
            break;
        }
        tokio::select! {
            // Fair data polling prevents a ready peer reader from starving
            // outcomes. Shutdown and expiry are also checked before every turn.
            _ = shutdown.changed() => break,
            frame = stream.reader.read() => {
                let Some(frame) = frame? else { break; };
                state.peer_activity(now(started)).map_err(io::Error::other)?;
                match frame {
                    Frame::Command(command) => authority.requests.try_send(owner::Request::Command { route, principal: principal.clone(), connection: authority.connection.clone(), message: ClientMessage::Command(command) }).map_err(|_| invalid("owner ingress full"))?,
                    Frame::Ping { nonce } => stream.writer.write(&Frame::Pong { nonce }).await?,
                    Frame::Pong { nonce } if ping == Some(nonce) => { ping = None; }
                    Frame::Close => break,
                    _ => return Err(invalid("unexpected gateway frame")),
                }
            }
            _ = tick.tick() => {
                let current = now(started);
                if current >= ping_at {
                    if ping.is_some() { break; }
                    ping = Some(current);
                    ping_at = current.saturating_add(10_000);
                    stream.writer.write(&Frame::Ping { nonce: current }).await?;
                }
            }
            outcome = responses.recv() => {
                let Some(outcome) = outcome else { break; };
                stream.writer.write(&Frame::Event(outcome)).await?;
            }
            changed = changes.changed() => {
                if changed.is_err() { break; }
                let snapshot = changes.borrow_and_update().clone();
                if let Some(snapshot) = snapshot { stream.writer.write(&Frame::Event(ServerMessage::Snapshot(snapshot))).await?; }
            }
        }
    }
    Ok(())
}

struct Authority {
    revoke: watch::Sender<bool>,
    requests: Pool,
    route: owner::Route,
    connection: String,
}
impl Drop for Authority {
    fn drop(&mut self) {
        self.revoke.send_replace(true);
        let _ = self.requests.try_send(owner::Request::Disconnect {
            route: self.route,
            connection: self.connection.clone(),
            reply: None,
        });
    }
}
