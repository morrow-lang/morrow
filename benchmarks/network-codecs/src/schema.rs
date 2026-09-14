//! Experimental numeric-field CBOR / protobuf schema. Tags are identical for both codecs.
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Wire {
    #[n(1)]
    #[prost(uint32, optional, tag = "1")]
    pub kind: Option<u32>,
    #[n(2)]
    #[prost(message, optional, tag = "2")]
    pub join: Option<Join>,
    #[n(3)]
    #[prost(message, optional, tag = "3")]
    pub command: Option<Command>,
    #[n(4)]
    #[prost(message, optional, tag = "4")]
    pub connected: Option<Connected>,
    #[n(5)]
    #[prost(message, optional, tag = "5")]
    pub snapshot: Option<Snapshot>,
    #[n(6)]
    #[prost(message, optional, tag = "6")]
    pub outcome: Option<Outcome>,
    #[n(7)]
    #[prost(string, optional, tag = "7")]
    pub error: Option<String>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Join {
    #[n(1)]
    #[prost(string, optional, tag = "1")]
    pub room: Option<String>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub resume_namespace: Option<String>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Command {
    #[n(1)]
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub incarnation: Option<String>,
    #[n(3)]
    #[prost(string, optional, tag = "3")]
    pub namespace: Option<String>,
    #[n(4)]
    #[prost(sint64, optional, tag = "4")]
    pub sequence: Option<i64>,
    #[n(5)]
    #[prost(sint64, optional, tag = "5")]
    pub expected_revision: Option<i64>,
    #[n(6)]
    #[prost(message, optional, tag = "6")]
    pub mutation: Option<Mutation>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Mutation {
    #[n(1)]
    #[prost(uint32, optional, tag = "1")]
    pub kind: Option<u32>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub label: Option<String>,
    #[n(3)]
    #[prost(sint64, optional, tag = "3")]
    pub id: Option<i64>,
    #[n(4)]
    #[prost(bool, optional, tag = "4")]
    pub done: Option<bool>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Connected {
    #[n(1)]
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub connection: Option<String>,
    #[n(3)]
    #[prost(string, optional, tag = "3")]
    pub namespace: Option<String>,
    #[n(4)]
    #[prost(sint64, optional, tag = "4")]
    pub next_sequence: Option<i64>,
    #[n(5)]
    #[prost(message, optional, tag = "5")]
    pub snapshot: Option<Snapshot>,
    #[n(6)]
    #[prost(bool, optional, tag = "6")]
    pub resumed: Option<bool>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Snapshot {
    #[n(1)]
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub room: Option<String>,
    #[n(3)]
    #[prost(string, optional, tag = "3")]
    pub incarnation: Option<String>,
    #[n(4)]
    #[prost(sint64, optional, tag = "4")]
    pub revision: Option<i64>,
    #[n(5)]
    #[prost(message, optional, tag = "5")]
    pub tasks: Option<TaskList>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct TaskList {
    #[n(1)]
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<Task>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Task {
    #[n(1)]
    #[prost(sint64, optional, tag = "1")]
    pub id: Option<i64>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub label: Option<String>,
    #[n(3)]
    #[prost(bool, optional, tag = "3")]
    pub done: Option<bool>,
}
#[derive(Clone, PartialEq, prost::Message, minicbor::Encode, minicbor::Decode)]
#[cbor(map)]
pub struct Outcome {
    #[n(1)]
    #[prost(uint32, optional, tag = "1")]
    pub version: Option<u32>,
    #[n(2)]
    #[prost(string, optional, tag = "2")]
    pub incarnation: Option<String>,
    #[n(3)]
    #[prost(string, optional, tag = "3")]
    pub namespace: Option<String>,
    #[n(4)]
    #[prost(sint64, optional, tag = "4")]
    pub sequence: Option<i64>,
    #[n(5)]
    #[prost(sint64, optional, tag = "5")]
    pub revision: Option<i64>,
    #[n(6)]
    #[prost(uint32, optional, tag = "6")]
    pub status: Option<u32>,
}
