//! Concrete codecs share one budget across parsing, conversion, path growth and output.
use super::*;
use crate::json_codec::{Direction, Kind as Wire, Plan};
mod containers;
mod sums;
mod unions;
struct Execution<'p, 'b> {
    plan: &'p Plan,
    budget: Budget<'b>,
    path: Rc<String>,
}
impl Machine {
    /// Evaluate the sole operand once, preserving normal propagation and cleanup behavior.
    pub(in crate::repl) fn json_codec(
        &mut self,
        direction: Direction,
        input: &ir::Expr,
        plan: &Plan,
    ) -> Eval<Value> {
        let input = self.expression(input)?;
        let limits = if self.cleanup_depth == 0 {
            &mut self.json_limits
        } else {
            &mut self.json_cleanup
        };
        match execute(direction, &input, plan, limits) {
            Ok(value) => Ok(Value::Sum(0, Rc::new(vec![value]))),
            Err(Error { code: 0, .. }) => Err(fault("interactive evaluation limit exceeded")),
            Err(error) => Ok(Value::Sum(1, Rc::new(vec![Value::JsonError(error)]))),
        }
    }
}
/// Reserve failure publication before descent; no failure path allocates additional path text.
fn execute(direction: Direction, input: &Value, plan: &Plan, limits: &mut Limits) -> Result<Value> {
    let mut budget = Budget {
        limits,
        work: 64 * 1024 * 1024,
        allocated: 0,
        nodes: 0,
        at: 0,
    };
    budget.allocate(128)?;
    let actual_publication = std::mem::size_of::<Value>() + 80;
    Limits::charge(
        &mut budget.limits.allocated,
        actual_publication.saturating_sub(128),
    )?;
    let mut execution = Execution {
        plan,
        budget,
        path: Rc::new(String::new()),
    };
    match direction {
        Direction::Encode => {
            let node = execution.encode(plan.root, input, 0)?;
            let result = (|| {
                execution.budget.work(node.encoded + node.nodes)?;
                execution.budget.allocate(node.encoded + 1)?;
                let mut text = String::with_capacity(node.encoded);
                value::encode(&node, &mut text);
                Ok(Value::String(Rc::new(text)))
            })();
            execution.locate(result)
        }
        Direction::Decode => {
            let Value::String(text) = input else {
                return Err(error(5, -1));
            };
            let text = source_text(text, &mut execution.budget).map_err(|e| Error {
                offset: INPUT as i64,
                ..e
            })?;
            execution.budget.work(8 * text.len())?;
            let node = parse::document_in(text, &mut execution.budget)?;
            execution.budget.at = 0;
            execution.decode(plan.root, &node, 0)
        }
    }
}
/// Charge every source byte before scanning for the native String terminator.
fn source_text<'s>(text: &'s str, budget: &mut Budget<'_>) -> Result<&'s str> {
    for (i, byte) in text.bytes().take(INPUT + 1).enumerate() {
        budget.work(1)?;
        if byte == 0 {
            return Ok(&text[..i]);
        }
    }
    if text.len() > INPUT {
        return Err(error(4, -1));
    }
    budget.work(1)?;
    Ok(text)
}
impl Execution<'_, '_> {
    /// Charge depth/work before dispatch and attach the already retained failing location once.
    fn encode(&mut self, id: usize, input: &Value, depth: usize) -> Result<Json> {
        self.step(depth)?;
        let result = self.encode_kind(id, input, depth);
        self.locate(result)
    }
    fn decode(&mut self, id: usize, input: &Json, depth: usize) -> Result<Value> {
        self.step(depth)?;
        let result = self.decode_kind(id, input, depth);
        self.locate(result)
    }
    /// Operation limits are conversion failures at the current path, not parser offsets.
    fn step(&mut self, depth: usize) -> Result<()> {
        if depth >= DEPTH {
            return self.locate(Err(error(4, -1)));
        }
        let result = self.budget.work(1).map_err(|e| Error { offset: -1, ..e });
        self.locate(result)
    }
    fn locate<T>(&self, result: Result<T>) -> Result<T> {
        result.map_err(|mut error| {
            if error.path.is_none() {
                if error.code == 4 {
                    error.offset = -1;
                }
                error.path = Some(self.path.clone());
            }
            error
        })
    }
    /// Reuse decimal conversion policy; known primitive nodes require no new parser or child allowance.
    fn encode_kind(&mut self, id: usize, input: &Value, depth: usize) -> Result<Json> {
        match (&self.plan.entries[id].kind, input) {
            (Wire::Union(children), Value::Union(value)) => {
                self.encode_union(children, value, depth)
            }
            (Wire::Newtype(child), _) => self.encode(*child, input, depth + 1),
            (Wire::Dynamic, Value::Json(node)) => {
                self.budget.work(node.nodes)?;
                Ok(node.clone())
            }
            (Wire::Unit, Value::Unit)
            | (Wire::Bool, Value::Bool(_))
            | (Wire::Int, Value::Int(_))
            | (Wire::Float, Value::Float(_)) => self.scalar(input),
            (Wire::String, Value::String(v)) => self.text(v),
            _ => self.encode_container(id, input, depth),
        }
    }
    /// Preserve exact dynamic integer/Float/text error semantics under the shared codec budget.
    fn decode_kind(&mut self, id: usize, input: &Json, depth: usize) -> Result<Value> {
        if matches!(
            self.plan.entries[id].kind,
            Wire::Int | Wire::Float | Wire::Bool | Wire::String
        ) {
            let work = match &input.kind {
                Kind::Number(v) | Kind::String(v) => v.len() * 2,
                _ => 1,
            };
            self.budget.work(work)?;
            self.budget.allocate(64)?;
        }
        match (&self.plan.entries[id].kind, &input.kind) {
            (Wire::Union(children), _) => self.decode_union(children, input, depth),
            (Wire::Newtype(child), _) => self.decode(*child, input, depth + 1),
            (Wire::Dynamic, _) => {
                self.budget.work(input.nodes)?;
                Ok(Value::Json(input.clone()))
            }
            (Wire::Unit, Kind::Null) => Ok(Value::Unit),
            (Wire::Bool, Kind::Bool(v)) => Ok(Value::Bool(*v)),
            (Wire::Int, Kind::Number(v)) => convert::integer(v).map(Value::Int),
            (Wire::Float, Kind::Number(v)) => convert::float(v).map(Value::Float),
            (Wire::String, Kind::String(v)) => self.string(v),
            _ => self.decode_container(id, input, depth),
        }
    }
    /// Reserve the same fixed native adapter envelope before formatting or allocation.
    fn scalar(&mut self, input: &Value) -> Result<Json> {
        self.budget.work(256)?;
        self.budget.allocate(256)?;
        if self.budget.nodes == NODES {
            return Err(error(4, -1));
        }
        self.budget.nodes += 1;
        let (kind, encoded) = match input {
            Value::Unit => (Kind::Null, 4),
            Value::Bool(value) => (Kind::Bool(*value), if *value { 4 } else { 5 }),
            Value::Int(value) => {
                let text = value.to_string();
                let n = text.len();
                (Kind::Number(text), n)
            }
            Value::Float(value) => {
                Limits::charge(&mut self.budget.limits.allocated, 512)?;
                let text = convert::format(*value)?;
                let n = text.len();
                (Kind::Number(text), n)
            }
            _ => return Err(error(5, -1)),
        };
        Ok(Rc::new(Node {
            kind,
            encoded,
            offset: 0,
            height: 1,
            nodes: 1,
        }))
    }
    fn text(&mut self, text: &str) -> Result<Json> {
        let text = source_text(text, &mut self.budget)?;
        self.budget.work(text.len() * 2)?;
        self.budget.node()?;
        self.budget.allocate(text.len() + 1)?;
        Ok(value::text_node(text.to_owned(), 0))
    }
    fn string(&mut self, text: &str) -> Result<Value> {
        if text.contains('\0') {
            return Err(error(10, -1));
        }
        // Native borrows the immutable DOM text; Rust requires a separate String allocation.
        Limits::charge(&mut self.budget.limits.allocated, text.len() + 41)?;
        Ok(Value::String(Rc::new(text.into())))
    }
    /// Reserve the full immutable pointer path before descent; failed growth reports the parent path.
    fn at<T>(&mut self, key: &str, call: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.path.len() >= OUTPUT || key.len() > (OUTPUT - self.path.len() - 1) / 2 {
            return self.locate(Err(error(4, -1)));
        }
        let length = self
            .path
            .len()
            .saturating_add(1)
            .saturating_add(key.len().saturating_mul(2));
        let reservation = (|| {
            self.budget.work(length)?;
            self.budget.allocate(length + 41)
        })();
        self.locate(reservation.map_err(|e| Error { offset: -1, ..e }))?;
        let mut path = String::with_capacity(length);
        path.push_str(&self.path);
        path.push('/');
        for ch in key.chars() {
            match ch {
                '~' => path.push_str("~0"),
                '/' => path.push_str("~1"),
                _ => path.push(ch),
            }
        }
        let previous = std::mem::replace(&mut self.path, Rc::new(path));
        let result = call(self);
        let result = self.locate(result);
        self.path = previous;
        result
    }
    /// Charge both native payload slots and larger interactive storage before allocating a container.
    fn slots(&mut self, count: usize) -> Result<()> {
        if count > NODES {
            return Err(error(4, -1));
        }
        self.budget.work(count)?;
        let native = count * 8 + 8;
        self.budget.allocate(native)?;
        let actual = count * std::mem::size_of::<Value>() + 40;
        Limits::charge(
            &mut self.budget.limits.allocated,
            actual.saturating_sub(native),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_adapter_reservations_match_native_before_conversion() {
        for (ty, kind, value) in [
            (Type::Int, Wire::Int, Value::Int(42)),
            (Type::Bool, Wire::Bool, Value::Bool(true)),
            (Type::Unit, Wire::Unit, Value::Unit),
        ] {
            let plan = Plan {
                root: 0,
                entries: vec![crate::json_codec::Entry { ty, kind }],
            };
            let mut limits = Limits::new(64 * 1024 * 1024);
            let budget = Budget {
                limits: &mut limits,
                work: 64 * 1024 * 1024,
                allocated: 128,
                nodes: 0,
                at: 0,
            };
            let mut execution = Execution {
                plan: &plan,
                budget,
                path: Rc::new(String::new()),
            };
            execution.encode(0, &value, 0).unwrap();
            assert_eq!(execution.budget.work, 64 * 1024 * 1024 - 257);
            assert_eq!(execution.budget.allocated, 128 + 256);
        }
    }
    #[test]
    fn siblings_share_the_same_native_execution_allowance() {
        let plan = Plan {
            root: 1,
            entries: vec![
                crate::json_codec::Entry {
                    ty: Type::Int,
                    kind: Wire::Int,
                },
                crate::json_codec::Entry {
                    ty: Type::List(Box::new(Type::Int)),
                    kind: Wire::List(0),
                },
            ],
        };
        let mut limits = Limits::new(64 * 1024 * 1024);
        let budget = Budget {
            limits: &mut limits,
            work: 500,
            allocated: 128,
            nodes: 0,
            at: 0,
        };
        let mut execution = Execution {
            plan: &plan,
            budget,
            path: Rc::new(String::new()),
        };
        let failure = execution
            .encode(
                1,
                &Value::List(Rc::new(vec![Value::Int(1), Value::Int(2)])),
                0,
            )
            .unwrap_err();
        assert_eq!(
            (
                failure.code,
                failure.offset,
                failure.path.as_deref().map(String::as_str)
            ),
            (4, -1, Some("/1"))
        );
        assert_eq!(
            (
                execution.budget.nodes,
                execution.budget.allocated,
                execution.budget.work
            ),
            (2, 602, 193)
        );
    }
    #[test]
    fn path_budget_failure_reports_parent_without_replacing_an_original_failure() {
        let plan = Plan {
            root: 0,
            entries: vec![crate::json_codec::Entry {
                ty: Type::Int,
                kind: Wire::Int,
            }],
        };
        let mut limits = Limits::new(64 * 1024 * 1024);
        let budget = Budget {
            limits: &mut limits,
            work: 64 * 1024 * 1024,
            allocated: ALLOC,
            nodes: 0,
            at: 0,
        };
        let mut execution = Execution {
            plan: &plan,
            budget,
            path: Rc::new("/parent".into()),
        };
        let failed = execution.at("child", |_| Ok(())).unwrap_err();
        assert_eq!(
            (
                failed.code,
                failed.offset,
                failed.path.as_deref().map(String::as_str)
            ),
            (4, -1, Some("/parent"))
        );
        let original = Error {
            code: 9,
            offset: 17,
            path: Some(Rc::new("/original".into())),
        };
        let retained = execution.locate::<()>(Err(original)).unwrap_err();
        assert_eq!(
            (
                retained.code,
                retained.offset,
                retained.path.as_deref().map(String::as_str)
            ),
            (9, 17, Some("/original"))
        );
    }
}
