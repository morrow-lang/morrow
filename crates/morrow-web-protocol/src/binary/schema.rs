//! Morrow live protobuf v1 DTOs. Field presence is checked before domain conversion.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Wire {
    #[prost(uint32, optional, tag = "1")]
    pub kind: Option<u32>,
    #[prost(message, optional, tag = "2")]
    pub join: Option<Join>,
    #[prost(message, optional, tag = "3")]
    pub command: Option<Command>,
    #[prost(message, optional, tag = "4")]
    pub connected: Option<Connected>,
    #[prost(message, optional, tag = "5")]
    pub snapshot: Option<Snapshot>,
    #[prost(message, optional, tag = "6")]
    pub outcome: Option<Outcome>,
    #[prost(string, optional, tag = "7")]
    pub error: Option<String>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Join {
    #[prost(string, optional, tag = "1")]
    pub room: Option<String>,
    #[prost(string, optional, tag = "2")]
    pub resume_namespace: Option<String>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Command {
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[prost(string, optional, tag = "2")]
    pub incarnation: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub namespace: Option<String>,
    #[prost(sint64, optional, tag = "4")]
    pub sequence: Option<i64>,
    #[prost(sint64, optional, tag = "5")]
    pub expected_revision: Option<i64>,
    #[prost(message, optional, tag = "6")]
    pub mutation: Option<Mutation>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Mutation {
    #[prost(uint32, optional, tag = "1")]
    pub kind: Option<u32>,
    #[prost(string, optional, tag = "2")]
    pub label: Option<String>,
    #[prost(sint64, optional, tag = "3")]
    pub id: Option<i64>,
    #[prost(bool, optional, tag = "4")]
    pub done: Option<bool>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Connected {
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[prost(string, optional, tag = "2")]
    pub connection: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub namespace: Option<String>,
    #[prost(sint64, optional, tag = "4")]
    pub next_sequence: Option<i64>,
    #[prost(message, optional, tag = "5")]
    pub snapshot: Option<Snapshot>,
    #[prost(bool, optional, tag = "6")]
    pub resumed: Option<bool>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Snapshot {
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[prost(string, optional, tag = "2")]
    pub room: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub incarnation: Option<String>,
    #[prost(sint64, optional, tag = "4")]
    pub revision: Option<i64>,
    #[prost(message, optional, tag = "5")]
    pub tasks: Option<TaskList>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct TaskList {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<Task>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Task {
    #[prost(sint64, optional, tag = "1")]
    pub id: Option<i64>,
    #[prost(string, optional, tag = "2")]
    pub label: Option<String>,
    #[prost(bool, optional, tag = "3")]
    pub done: Option<bool>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Outcome {
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[prost(string, optional, tag = "2")]
    pub incarnation: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub namespace: Option<String>,
    #[prost(sint64, optional, tag = "4")]
    pub sequence: Option<i64>,
    #[prost(sint64, optional, tag = "5")]
    pub revision: Option<i64>,
    #[prost(uint32, optional, tag = "6")]
    pub status: Option<u32>,
}
