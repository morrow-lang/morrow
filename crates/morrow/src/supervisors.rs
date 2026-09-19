//! Closed supervisor schemas and typed operations; runtime authority stays private.
use crate::{Type, ir, runtime::NativeType};

pub(crate) const API_NAMES: &[&str] = &[
    "Supervisor.child_key", "Supervisor.worker", "Supervisor.branch", "Supervisor.id",
    "Supervisor.start", "Supervisor.start_link", "Supervisor.current", "Supervisor.stop",
    "Process.init_ack", "Process.init_ignore", "Process.init_fail",
];
pub(crate) fn is_api(name: &str) -> bool { API_NAMES.contains(&name) }
pub(crate) fn contextual(name: &str) -> bool {
    matches!(name, "Supervisor.start" | "Supervisor.current" | "Supervisor.stop")
}
pub(crate) fn requires_actor(name: &str) -> bool {
    matches!(name, "Supervisor.start_link" | "Process.init_ack" | "Process.init_ignore" | "Process.init_fail")
}
pub(crate) fn named(name: &str) -> Type { Type::Named(format!("Supervisor.{name}"), vec![]) }
pub(crate) fn handle() -> Type { Type::Native(NativeType::SupervisorHandle) }
pub(crate) fn spec() -> Type { Type::Native(NativeType::ChildSpec) }
pub(crate) fn result(ty: Type) -> Type { Type::Result(Box::new(ty), Box::new(named("Error"))) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind { Start, StartLink, Current, Stop }
impl RequestKind {
    pub(crate) fn opcode(self) -> i64 { match self { Self::Start => 0, Self::StartLink => 1, Self::Current => 2, Self::Stop => 3 } }
}
#[derive(Clone, Debug)]
pub enum SupervisorExpr {
    ChildKey { name: Box<ir::Expr>, mailbox: Type },
    Worker { key: Box<ir::Expr>, entry: Box<ir::Expr>, policy: Box<ir::Expr>, mailbox: Type },
    Branch { name: Box<ir::Expr>, flags: Box<ir::Expr>, children: Box<ir::Expr>, policy: Box<ir::Expr> },
    Id { handle: Box<ir::Expr> },
    Request { kind: RequestKind, args: Vec<ir::Expr>, mailbox: Option<Type> },
    InitAck,
    InitIgnore,
    InitFail { reason: Box<ir::Expr> },
}
impl SupervisorExpr {
    pub(crate) fn children(&self) -> Vec<&ir::Expr> {
        match self {
            Self::ChildKey { name, .. } => vec![name],
            Self::Worker { key, entry, policy, .. } => vec![key, entry, policy],
            Self::Branch { name, flags, children, policy } => vec![name, flags, children, policy],
            Self::Id { handle } => vec![handle],
            Self::Request { args, .. } => args.iter().collect(),
            Self::InitFail { reason } => vec![reason],
            Self::InitAck | Self::InitIgnore => vec![],
        }
    }
    pub(crate) fn children_mut(&mut self) -> Vec<&mut ir::Expr> {
        match self {
            Self::ChildKey { name, .. } => vec![name],
            Self::Worker { key, entry, policy, .. } => vec![key, entry, policy],
            Self::Branch { name, flags, children, policy } => vec![name, flags, children, policy],
            Self::Id { handle } => vec![handle],
            Self::Request { args, .. } => args.iter_mut().collect(),
            Self::InitFail { reason } => vec![reason],
            Self::InitAck | Self::InitIgnore => vec![],
        }
    }
    pub(crate) fn terminal(&self) -> bool { matches!(self, Self::InitIgnore | Self::InitFail { .. }) }
}

/// One canonical schema source for nominal registration and public-IR validation.
pub(crate) struct Schema {
    pub name: &'static str,
    pub variants: Vec<(&'static str, Vec<Type>)>,
    pub fields: Vec<&'static str>,
}
pub(crate) fn schemas() -> Vec<Schema> {
    let enums = [
        ("Strategy", vec![("OneForOne", vec![]), ("OneForAll", vec![]), ("RestForOne", vec![])]),
        ("Restart", vec![("Permanent", vec![]), ("Transient", vec![]), ("Temporary", vec![])]),
        ("Shutdown", vec![("Graceful", vec![Type::Int]), ("Infinity", vec![]), ("Immediate", vec![])]),
        ("AutoShutdown", vec![("Never", vec![]), ("AnySignificant", vec![]), ("AllSignificant", vec![])]),
        ("Error", vec![("InvalidOptions", vec![]), ("ResourceLimit", vec![]), ("ForeignInvocation", vec![]), ("UnsupportedContext", vec![]), ("WrongChildKey", vec![]), ("DuplicateName", vec![]), ("AlreadyRunning", vec![]), ("AlreadyPresent", vec![]), ("Restarting", vec![]), ("Stopped", vec![]), ("Removed", vec![]), ("StartFailed", vec![Type::String, crate::processes::reason()]), ("SupervisorStopped", vec![])]),
        ("ChildState", vec![("Running", vec![crate::processes::identity()]), ("Stopped", vec![]), ("Restarting", vec![])]),
        ("ChildKind", vec![("Worker", vec![]), ("Branch", vec![])]),
    ];
    let mut schemas: Vec<_> = enums.into_iter().map(|(name, variants)| Schema { name, variants, fields: vec![] }).collect();
    for (name, fields, types) in [
        ("Flags", vec!["strategy", "intensity", "period_seconds", "auto_shutdown"], vec![named("Strategy"), Type::Int, Type::Int, named("AutoShutdown")]),
        ("ChildPolicy", vec!["restart", "shutdown", "significant"], vec![named("Restart"), named("Shutdown"), Type::Bool]),
        ("ChildInfo", vec!["name", "kind", "state"], vec![Type::String, named("ChildKind"), named("ChildState")]),
    ] { schemas.push(Schema { name, variants: vec![(name, types)], fields }); }
    schemas
}
pub(crate) fn nominal_shape(ty: &Type) -> Option<(Vec<Vec<Type>>, Vec<String>)> {
    let Type::Named(name, args) = ty else { return None; };
    if !args.is_empty() { return None; }
    schemas().into_iter().find(|schema| name == &format!("Supervisor.{}", schema.name))
        .map(|schema| (schema.variants.into_iter().map(|(_, fields)| fields).collect(), schema.fields.into_iter().map(str::to_owned).collect()))
}
pub(crate) fn nominal_name(name: &str) -> bool {
    schemas().iter().any(|schema| name == format!("Supervisor.{}", schema.name))
}
pub(crate) fn constructor_path(name: &str) -> bool {
    schemas().iter().any(|schema| schema.variants.iter().any(|(variant, _)|
        name == format!("Supervisor.{variant}") || name == format!("Supervisor.{}.{variant}", schema.name)))
}

/// Private descriptor schema; no source constructor or public operation accepts this type.
pub(crate) fn request_type(kind: RequestKind, args: &[ir::Expr]) -> Type {
    let (name, parameters) = match kind {
        RequestKind::Start | RequestKind::StartLink => ("$Supervisor.Start", vec![]),
        RequestKind::Stop => ("$Supervisor.Stop", vec![]),
        RequestKind::Current => ("$Supervisor.Current", match args.get(1).map(|a| &a.ty) { Some(Type::ChildKey(mailbox)) => vec![*mailbox.clone()], _ => vec![] }),
    };
    Type::Named(name.into(), parameters)
}
pub(crate) fn request_fields(ty: &Type) -> Option<Vec<Type>> {
    let Type::Named(name, parameters) = ty else { return None; };
    match (name.as_str(), parameters.as_slice()) {
        ("$Supervisor.Start", []) => Some(vec![named("Flags"), Type::List(Box::new(spec()))]),
        ("$Supervisor.Stop", []) => Some(vec![handle()]),
        ("$Supervisor.Current", [mailbox]) => Some(vec![handle(), Type::ChildKey(Box::new(mailbox.clone()))]),
        _ => None,
    }
}
