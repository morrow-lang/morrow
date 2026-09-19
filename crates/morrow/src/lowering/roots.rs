//! Compiler-owned root slots retain managed references before machine widths erase
//! their identities. The runtime still scans legacy helper/native boundaries;
//! this establishes explicit roots without claiming globally precise collection.
use super::*;

const MAX_ROOTS: usize = 65_536;

#[derive(Default)]
pub(super) struct Frame {
    slots: BTreeMap<String, usize>,
    exceeded: bool,
}

impl Emitter<'_> {
    /// Newtypes follow their validated payload representation. PID identities
    /// are heap wrappers, and therefore remain roots even though IDs are scalar.
    pub(super) fn root_value(&mut self, locals: &mut Locals, ty: &Type, value: &str) {
        if !matches!(
            self.representation(ty),
            Type::Int | Type::Float | Type::Bool | Type::Unit | Type::Never
        ) {
            self.root_pointer(locals, value);
        }
    }

    /// Retain an implementation pointer with known allocation/environment provenance.
    /// This is never inferred from a source Int or a machine register's width.
    pub(super) fn root_pointer(&mut self, locals: &mut Locals, value: &str) {
        // Static descriptors/literals and null are not collector allocations.
        if !matches!(native_operand(value), Operand::Temp(_)) {
            return;
        }
        let index = if let Some(index) = locals.roots.slots.get(value) {
            *index
        } else {
            if locals.roots.slots.len() == MAX_ROOTS {
                locals.roots.exceeded = true;
                return;
            }
            let index = locals.roots.slots.len();
            locals.roots.slots.insert(value.into(), index);
            index
        };
        // Repeated reads can occur on distinct control-flow paths. Always write
        // the slot here rather than assuming a previous path executed its store.
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(value),
            address: native_operand(&format!("%gc_root{index}")),
        });
    }

    /// Reserve a writable, initialized word in the callback's already-registered native frame.
    pub(super) fn reply_root(&self, locals: &mut Locals, span: Span) -> Lowering<String> {
        if locals.roots.slots.len() >= MAX_ROOTS { return Err(invalid(span, "native root slot limit exceeded")); }
        let index = locals.roots.slots.len();
        locals.roots.slots.insert(format!("$supervisor_reply{index}"), index);
        Ok(format!("%gc_root{index}"))
    }

    /// Reserve and zero every fixed slot before registration or any allocating body.
    pub(super) fn root_entry(&self, locals: &mut Locals, span: Span) -> Lowering<()> {
        if locals.roots.exceeded {
            return Err(invalid(span, "native root slot limit exceeded"));
        }
        let count = locals.roots.slots.len();
        if count == 0 {
            return Ok(());
        }
        locals.stack_allocations.statement(Statement::Assign {
            destination: "%gc_slots".into(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc {
                bytes: (count * 8) as u32,
                align: 8,
            },
        });
        for index in 0..count {
            let address = format!("%gc_root{index}");
            locals.stack_allocations.statement(Statement::Assign {
                destination: address.clone(),
                ty: Scalar::I64,
                operation: NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand("%gc_slots"),
                    Operand::Int((index * 8) as i64),
                ),
            });
            locals.stack_allocations.statement(Statement::Store {
                kind: LoadKind::I64,
                value: Operand::Int(0),
                address: native_operand(&address),
            });
        }
        locals.stack_allocations.statement(Statement::Assign {
            destination: "%gc_frame".into(),
            ty: Scalar::I64,
            operation: NativeOperation::Call {
                callee: native_operand("$morrow_gc_frame_enter"),
                args: vec![
                    (Scalar::I64, native_operand("%gc_slots")),
                    (Scalar::I64, Operand::Int(count as i64)),
                ],
                variadic: None,
            },
        });
        Ok(())
    }

    /// Defer execution finishes before the original heap's registration retires.
    pub(super) fn root_exit(&mut self, locals: &Locals) {
        if locals.roots.slots.is_empty() {
            return;
        }
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_gc_frame_leave"),
                args: vec![(Scalar::I64, native_operand("%gc_frame"))],
                variadic: None,
            }));
    }

    /// Root stores never allocate. Move them after a complete leading phi group
    /// so explicit retention does not violate the machine IR's SSA block shape.
    pub(super) fn root_phis(&mut self, start: usize) {
        let items = &mut self.output.items;
        let mut cursor = start;
        while cursor < items.len() {
            if !matches!(items[cursor], machine::Item::Statement(Statement::Label(_))) {
                cursor += 1;
                continue;
            }
            cursor += 1;
            let begin = cursor;
            while cursor < items.len() && (is_phi(&items[cursor]) || is_root_store(&items[cursor]))
            {
                cursor += 1;
            }
            items[begin..cursor].sort_by_key(|item| usize::from(!is_phi(item)));
        }
    }
}

fn is_phi(item: &machine::Item) -> bool {
    matches!(
        item,
        machine::Item::Statement(Statement::Assign {
            operation: NativeOperation::Phi(_),
            ..
        })
    )
}

fn is_root_store(item: &machine::Item) -> bool {
    matches!(item, machine::Item::Statement(Statement::Store { address: Operand::Temp(name), .. }) if name.starts_with("gc_root"))
}
