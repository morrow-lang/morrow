//! Shared bounded JSON semantics for native and interactive execution.
#![forbid(unsafe_code)]
use std::rc::Rc;
pub mod convert;
pub mod parse;
mod retained;
pub mod scope;
mod value;
pub use retained::retained_bytes;
pub use value::{encode, encode_string, get, seal, stringify, text_node};
pub const INPUT: usize = 1_048_576;
pub const OUTPUT: usize = 16_777_216;
pub const ALLOC: usize = 33_554_432;
pub const NODES: usize = 100_000;
pub const DEPTH: usize = 128;
pub type Json = Rc<Node>;
#[derive(Debug)]
pub struct Node {
    pub kind: Kind,
    pub offset: usize,
    pub height: usize,
    pub nodes: usize,
    pub encoded: usize,
}
#[derive(Debug)]
pub enum Kind {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(Json, Json)>, Vec<usize>),
}
/// Opaque values are never compared structurally, even by internal evaluator helpers.
impl PartialEq for Node {
    /// Compare node identities only; opaque JSON never gains structural source equality.
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub path: Option<Rc<String>>,
    pub code: u8,
    pub offset: i64,
}
pub type Result<T> = std::result::Result<T, Error>;
/// Construct a stable code/byte-offset failure; code zero is reserved for evaluator exhaustion.
pub fn error(code: u8, offset: i64) -> Error {
    Error {
        code,
        offset,
        path: None,
    }
}
/// Return the native NUL-terminated prefix, scanning no farther than the input limit plus one.
pub fn input(text: &str) -> &str {
    let end = text
        .bytes()
        .take(INPUT + 1)
        .position(|b| b == 0)
        .unwrap_or(text.len());
    &text[..end]
}

#[derive(Debug)]
pub struct Limits {
    pub work: usize,
    pub allocated: usize,
}
impl Limits {
    /// Initialize independent work/allocation allowances of `bytes` for one evaluation phase.
    pub fn new(bytes: usize) -> Self {
        Self {
            work: bytes,
            allocated: bytes,
        }
    }
    /// Consume `amount` before work or allocation; exhaustion empties the allowance and returns a fault.
    pub fn charge(left: &mut usize, amount: usize) -> Result<()> {
        if amount > *left {
            *left = 0;
            scope::mark_exhausted();
            return Err(error(0, -1));
        }
        *left -= amount;
        Ok(())
    }
}
pub struct Budget<'a> {
    pub limits: &'a mut Limits,
    pub work: usize,
    pub allocated: usize,
    pub nodes: usize,
    pub at: usize,
}
impl<'a> Budget<'a> {
    /// Reserve bounded input-scan work in a native-profile operation linked to aggregate limits.
    pub fn new(limits: &'a mut Limits, bytes: usize) -> Result<Self> {
        let mut budget = Self {
            limits,
            work: 8 * bytes + 64 * NODES,
            allocated: 0,
            nodes: 0,
            at: 0,
        };
        budget.work(8 * bytes)?;
        Ok(budget)
    }
    /// Charge aggregate and operation work before executing `units` of bounded processing.
    pub fn work(&mut self, units: usize) -> Result<()> {
        Limits::charge(&mut self.limits.work, units)?;
        if units > self.work {
            return Err(error(4, self.at as i64));
        }
        self.work -= units;
        Ok(())
    }
    /// Charge logical native bytes before allocation; zero-byte requests retain the native one-byte rule.
    pub fn allocate(&mut self, bytes: usize) -> Result<()> {
        let bytes = bytes.max(1);
        Limits::charge(&mut self.limits.allocated, bytes)?;
        if bytes > ALLOC - self.allocated {
            return Err(error(4, self.at as i64));
        }
        self.allocated += bytes;
        Ok(())
    }
    /// Reserve one native-profile node plus larger Rust storage before publishing its Rc allocation.
    pub fn node(&mut self) -> Result<()> {
        scope::charge_nodes(1)?;
        if self.nodes == NODES {
            return Err(error(4, self.at as i64));
        }
        self.allocate(72)?;
        let actual = std::mem::size_of::<Node>() + 2 * std::mem::size_of::<usize>();
        Limits::charge(&mut self.limits.allocated, actual.saturating_sub(72))?;
        self.nodes += 1;
        Ok(())
    }
}

/// Stable JSON error messages shared by all execution paths.
pub fn message(code: u8) -> &'static str {
    [
        "",
        "invalid JSON syntax",
        "invalid JSON Unicode",
        "duplicate JSON object key",
        "JSON resource limit exceeded",
        "JSON value has wrong type",
        "JSON object key not found",
        "JSON array index out of bounds",
        "JSON number out of range",
        "JSON number is not an integer",
        "JSON string contains NUL",
        "JSON number is not finite",
        "unknown JSON object field",
        "unknown JSON variant",
        "no unique JSON union member",
    ]
    .get(code as usize)
    .copied()
    .unwrap_or("invalid JSON error")
}
