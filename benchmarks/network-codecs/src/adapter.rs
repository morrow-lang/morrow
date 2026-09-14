use crate::{Message, protocol as p, schema as w};
type Checked<T> = Result<T, String>;
fn need<T>(v: Option<T>) -> Checked<T> {
    v.ok_or_else(|| "missing required field".into())
}
fn version(v: Option<u32>) -> Checked<u8> {
    u8::try_from(need(v)?).map_err(|_| "version overflow".into())
}
fn decimal(v: Option<i64>) -> Checked<p::Decimal> {
    Ok(p::Decimal(need(v)?))
}
impl From<&Message> for w::Wire {
    fn from(value: &Message) -> Self {
        let mut wire = Self::default();
        match value {
            Message::Client(p::ClientMessage::Join {
                room,
                resume_namespace,
            }) => {
                wire.kind = Some(1);
                wire.join = Some(w::Join {
                    room: Some(room.clone()),
                    resume_namespace: resume_namespace.clone(),
                });
            }
            Message::Client(p::ClientMessage::Command(command)) => {
                wire.kind = Some(2);
                wire.command = Some(command.into());
            }
            Message::Server(p::ServerMessage::Connected(c)) => {
                wire.kind = Some(3);
                wire.connected = Some(w::Connected {
                    version: Some(c.version.into()),
                    connection: Some(c.connection.clone()),
                    namespace: Some(c.namespace.clone()),
                    next_sequence: Some(c.next_sequence.0),
                    snapshot: Some((&c.snapshot).into()),
                    resumed: Some(c.resumed),
                });
            }
            Message::Server(p::ServerMessage::Snapshot(s)) => {
                wire.kind = Some(4);
                wire.snapshot = Some(s.into());
            }
            Message::Server(p::ServerMessage::Reset(s)) => {
                wire.kind = Some(5);
                wire.snapshot = Some(s.into());
            }
            Message::Server(p::ServerMessage::Outcome(o)) => {
                wire.kind = Some(6);
                wire.outcome = Some(o.into());
            }
            Message::Server(p::ServerMessage::Error(error)) => {
                wire.kind = Some(7);
                wire.error = Some(
                    serde_json::to_value(error)
                        .expect("infallible enum serialization")
                        .as_str()
                        .expect("string enum")
                        .into(),
                );
            }
        }
        wire
    }
}
impl From<&p::Command> for w::Command {
    fn from(v: &p::Command) -> Self {
        Self {
            version: Some(v.version.into()),
            incarnation: Some(v.incarnation.clone()),
            namespace: Some(v.namespace.clone()),
            sequence: Some(v.sequence.0),
            expected_revision: Some(v.expected_revision.0),
            mutation: Some((&v.mutation).into()),
        }
    }
}
impl From<&p::Mutation> for w::Mutation {
    fn from(v: &p::Mutation) -> Self {
        let mut wire = Self::default();
        match v {
            p::Mutation::Add { label } => {
                wire.kind = Some(1);
                wire.label = Some(label.clone());
            }
            p::Mutation::SetDone { id, done } => {
                wire.kind = Some(2);
                wire.id = Some(id.0);
                wire.done = Some(*done);
            }
            p::Mutation::Remove { id } => {
                wire.kind = Some(3);
                wire.id = Some(id.0);
            }
        }
        wire
    }
}
impl From<&p::Snapshot> for w::Snapshot {
    fn from(v: &p::Snapshot) -> Self {
        Self {
            version: Some(v.version.into()),
            room: Some(v.room.clone()),
            incarnation: Some(v.incarnation.clone()),
            revision: Some(v.revision.0),
            tasks: Some(w::TaskList {
                items: v
                    .tasks
                    .iter()
                    .map(|t| w::Task {
                        id: Some(t.id.0),
                        label: Some(t.label.clone()),
                        done: Some(t.done),
                    })
                    .collect(),
            }),
        }
    }
}
impl From<&p::Outcome> for w::Outcome {
    fn from(v: &p::Outcome) -> Self {
        Self {
            version: Some(v.version.into()),
            incarnation: Some(v.incarnation.clone()),
            namespace: Some(v.namespace.clone()),
            sequence: Some(v.sequence.0),
            revision: Some(v.revision.0),
            status: Some(match v.status {
                p::Status::Applied => 1,
                p::Status::Conflict => 2,
                p::Status::NotFound => 3,
                p::Status::Capacity => 4,
                p::Status::Unknown => 5,
            }),
        }
    }
}
impl TryFrom<w::Wire> for Message {
    type Error = String;
    fn try_from(mut w: w::Wire) -> Checked<Self> {
        if [
            w.join.is_some(),
            w.command.is_some(),
            w.connected.is_some(),
            w.snapshot.is_some(),
            w.outcome.is_some(),
            w.error.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
            != 1
        {
            return Err("envelope must carry exactly one payload".into());
        }
        Ok(match need(w.kind)? {
            1 => {
                let j = need(w.join.take())?;
                Self::Client(p::ClientMessage::Join {
                    room: need(j.room)?,
                    resume_namespace: j.resume_namespace,
                })
            }
            2 => Self::Client(p::ClientMessage::Command(
                need(w.command.take())?.try_into()?,
            )),
            3 => {
                let c = need(w.connected.take())?;
                Self::Server(p::ServerMessage::Connected(p::Connected {
                    version: version(c.version)?,
                    connection: need(c.connection)?,
                    namespace: need(c.namespace)?,
                    next_sequence: decimal(c.next_sequence)?,
                    snapshot: need(c.snapshot)?.try_into()?,
                    resumed: need(c.resumed)?,
                }))
            }
            4 => Self::Server(p::ServerMessage::Snapshot(
                need(w.snapshot.take())?.try_into()?,
            )),
            5 => Self::Server(p::ServerMessage::Reset(
                need(w.snapshot.take())?.try_into()?,
            )),
            6 => Self::Server(p::ServerMessage::Outcome(
                need(w.outcome.take())?.try_into()?,
            )),
            7 => Self::Server(p::ServerMessage::Error(
                serde_json::from_value(serde_json::Value::String(need(w.error)?))
                    .map_err(|e| e.to_string())?,
            )),
            _ => return Err("unknown envelope kind".into()),
        })
    }
}
impl TryFrom<w::Command> for p::Command {
    type Error = String;
    fn try_from(v: w::Command) -> Checked<Self> {
        Ok(Self {
            version: version(v.version)?,
            incarnation: need(v.incarnation)?,
            namespace: need(v.namespace)?,
            sequence: decimal(v.sequence)?,
            expected_revision: decimal(v.expected_revision)?,
            mutation: need(v.mutation)?.try_into()?,
        })
    }
}
impl TryFrom<w::Mutation> for p::Mutation {
    type Error = String;
    fn try_from(v: w::Mutation) -> Checked<Self> {
        match need(v.kind)? {
            1 if v.id.is_none() && v.done.is_none() => Ok(Self::Add {
                label: need(v.label)?,
            }),
            2 if v.label.is_none() => Ok(Self::SetDone {
                id: decimal(v.id)?,
                done: need(v.done)?,
            }),
            3 if v.label.is_none() && v.done.is_none() => Ok(Self::Remove { id: decimal(v.id)? }),
            _ => Err("invalid mutation fields".into()),
        }
    }
}
impl TryFrom<w::Snapshot> for p::Snapshot {
    type Error = String;
    fn try_from(v: w::Snapshot) -> Checked<Self> {
        Ok(Self {
            version: version(v.version)?,
            room: need(v.room)?,
            incarnation: need(v.incarnation)?,
            revision: decimal(v.revision)?,
            tasks: need(v.tasks)?
                .items
                .into_iter()
                .map(|t| {
                    Ok(p::Task {
                        id: decimal(t.id)?,
                        label: need(t.label)?,
                        done: need(t.done)?,
                    })
                })
                .collect::<Checked<_>>()?,
        })
    }
}
impl TryFrom<w::Outcome> for p::Outcome {
    type Error = String;
    fn try_from(v: w::Outcome) -> Checked<Self> {
        Ok(Self {
            version: version(v.version)?,
            incarnation: need(v.incarnation)?,
            namespace: need(v.namespace)?,
            sequence: decimal(v.sequence)?,
            revision: decimal(v.revision)?,
            status: match need(v.status)? {
                1 => p::Status::Applied,
                2 => p::Status::Conflict,
                3 => p::Status::NotFound,
                4 => p::Status::Capacity,
                5 => p::Status::Unknown,
                _ => return Err("unknown outcome".into()),
            },
        })
    }
}
