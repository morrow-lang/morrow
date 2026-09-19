//! Typed process identities and the closed source API shared by checking and lowering.
use crate::{Type, ir};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReceiveView {
    #[default]
    Messages,
    Events,
}
impl ReceiveView {
    pub(crate) fn item(self, mailbox: &Type) -> Type {
        match self {
            Self::Messages => mailbox.clone(),
            Self::Events => event(mailbox.clone()),
        }
    }
}
pub(crate) const API_NAMES: &[&str] = &[
    "Process.spawn",
    "Process.spawn_monitor",
    "Process.self",
    "Process.id",
    "Process.monitor",
    "Process.demonitor",
];
pub(crate) fn is_api(name: &str) -> bool {
    API_NAMES.contains(&name)
}
pub(crate) fn requires_actor(name: &str) -> bool {
    is_api(name) && !matches!(name, "Process.spawn" | "Process.id")
}
pub(crate) fn identity() -> Type {
    Type::Native(crate::runtime::NativeType::ProcessId)
}
pub(crate) fn monitor_ref() -> Type {
    Type::Native(crate::runtime::NativeType::MonitorRef)
}
pub(crate) fn opaque(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Native(
            crate::runtime::NativeType::ProcessId | crate::runtime::NativeType::MonitorRef
        )
    )
}
pub(crate) fn event(mailbox: Type) -> Type {
    Type::Named("Process.Event".into(), vec![mailbox])
}
pub(crate) fn error() -> Type {
    Type::Named("Process.Error".into(), vec![])
}
pub(crate) fn reason() -> Type {
    Type::Named("Process.ExitReason".into(), vec![])
}
pub(crate) fn options() -> Type {
    Type::Named("Process.DemonitorOptions".into(), vec![])
}
pub(crate) fn result(value: Type) -> Type {
    Type::Result(Box::new(value), Box::new(error()))
}

/// Public typed operations are independently validated before native emission.
#[derive(Clone, Debug)]
pub enum ProcessExpr {
    Spawn {
        entry: Box<ir::Expr>,
        mailbox: Type,
        monitor: bool,
    },
    SelfPid {
        mailbox: Type,
    },
    Id {
        pid: Box<ir::Expr>,
    },
    Monitor {
        target: Box<ir::Expr>,
    },
    Demonitor {
        reference: Box<ir::Expr>,
        options: Box<ir::Expr>,
    },
}
impl ProcessExpr {
    pub(crate) fn children(&self) -> Vec<&ir::Expr> {
        match self {
            Self::Spawn { entry, .. } => vec![entry],
            Self::SelfPid { .. } => vec![],
            Self::Id { pid } => vec![pid],
            Self::Monitor { target } => vec![target],
            Self::Demonitor { reference, options } => vec![reference, options],
        }
    }
    pub(crate) fn children_mut(&mut self) -> Vec<&mut ir::Expr> {
        match self {
            Self::Spawn { entry, .. } => vec![entry],
            Self::SelfPid { .. } => vec![],
            Self::Id { pid } => vec![pid],
            Self::Monitor { target } => vec![target],
            Self::Demonitor { reference, options } => vec![reference, options],
        }
    }
}

/// Closed nominal schemas also validate callers that construct public typed IR directly.
pub(crate) fn nominal_shape(ty: &Type) -> Option<(Vec<Vec<Type>>, Vec<String>)> {
    let Type::Named(name, args) = ty else {
        return None;
    };
    let variants = match (name.as_str(), args.as_slice()) {
        ("Process.Event", [mailbox]) => vec![
            vec![mailbox.clone()],
            vec![monitor_ref(), identity(), reason()],
            vec![identity(), reason()],
        ],
        ("Process.ExitReason", []) => vec![
            vec![],
            vec![],
            vec![Type::String],
            vec![Type::Int],
            vec![Type::String],
            vec![],
            vec![],
            vec![],
        ],
        ("Process.Error", []) => vec![vec![]; 5],
        ("Process.DemonitorOptions", []) => {
            return Some((
                vec![vec![Type::Bool, Type::Bool]],
                vec!["flush".into(), "info".into()],
            ));
        }
        _ => return None,
    };
    Some((variants, vec![]))
}
pub(crate) fn nominal_name(name: &str) -> bool {
    matches!(
        name,
        "Process.Event" | "Process.ExitReason" | "Process.Error" | "Process.DemonitorOptions"
    )
}
pub(crate) fn constructor_path(name: &str) -> bool {
    let Some((owner, variant)) = name.rsplit_once('.') else {
        return false;
    };
    match owner {
        "Process" => matches!(
            variant,
            "Message"
                | "Down"
                | "Exit"
                | "Normal"
                | "Shutdown"
                | "ShutdownDetail"
                | "Fault"
                | "Failure"
                | "Kill"
                | "Killed"
                | "NoProcess"
                | "ResourceLimit"
                | "ForeignInvocation"
                | "WrongMonitorOwner"
                | "UnsupportedTarget"
                | "InvalidOptions"
                | "DemonitorOptions"
        ),
        "Process.Event" => matches!(variant, "Message" | "Down" | "Exit"),
        "Process.ExitReason" => matches!(
            variant,
            "Normal"
                | "Shutdown"
                | "ShutdownDetail"
                | "Fault"
                | "Failure"
                | "Kill"
                | "Killed"
                | "NoProcess"
        ),
        "Process.Error" => matches!(
            variant,
            "ResourceLimit"
                | "ForeignInvocation"
                | "WrongMonitorOwner"
                | "UnsupportedTarget"
                | "InvalidOptions"
        ),
        _ => false,
    }
}
