//! Emit validated concrete wire descriptors and one source-ordered runtime operation.
use super::*;
use crate::json_codec::{Direction, Kind, Plan};
impl Emitter<'_> {
    /// Preserve the operand's full payload width; runtime errors remain ordinary heap Results.
    pub(super) fn json_codec(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let ExprKind::JsonCodec {
            direction,
            input,
            plan,
        } = &expr.kind
        else {
            return Err(invalid(expr.span, "expected JSON codec operation"));
        };
        let value = self.expr(input, locals, depth)?;
        let value = self.payload(locals, &input.ty, value);
        let root = self.codec_table(plan, expr.span)?;
        let name = match direction {
            Direction::Encode => "encode",
            Direction::Decode => "decode",
        };
        let value = self.assign(
            locals,
            expr.ty.clone(),
            NativeOperation::Call {
                callee: native_operand(&format!("$fern_json_codec_{}", name)),
                args: vec![
                    (Scalar::I64, native_operand(&(root))),
                    (Scalar::I64, native_operand(&(value))),
                ],
                variadic: None,
            },
        );
        Ok((expr.ty.clone(), value))
    }
    /// Bound aggregate descriptor/name output before allocating formatting intermediates.
    fn reserve_codec_data(&mut self, plan: &Plan, span: Span) -> Lowering<()> {
        let mut bytes = 0usize;
        for entry in &plan.entries {
            bytes = bytes.saturating_add(256);
            match &entry.kind {
                Kind::Sum(variants) => {
                    for variant in variants {
                        bytes = bytes
                            .saturating_add(variant.wire_tag.len().saturating_mul(8))
                            .saturating_add(512)
                            .saturating_add(variant.fields.len().saturating_mul(64));
                    }
                }
                Kind::Tuple(ids) | Kind::Union(ids) => {
                    bytes = bytes.saturating_add(ids.len().saturating_mul(64))
                }
                Kind::Record(fields) => {
                    for field in fields {
                        bytes = bytes
                            .saturating_add(field.name.len().saturating_mul(8).saturating_add(512));
                    }
                }
                _ => bytes = bytes.saturating_add(64),
            }
        }
        self.codec_data_bytes = self.codec_data_bytes.saturating_add(bytes);
        if self.codec_data_bytes > 16 * 1024 * 1024 {
            return Err(invalid(span, "JSON codec descriptor output limit exceeded"));
        }
        Ok(())
    }
    /// All descriptors use four native 64-bit words and validated finite-graph child pointers.
    fn codec_table(&mut self, plan: &std::rc::Rc<Plan>, span: Span) -> Lowering<String> {
        let identity = std::rc::Rc::as_ptr(plan) as usize;
        if let Some(table) = self.codec_tables.get(&identity) {
            return Ok(table.clone());
        }
        self.reserve_codec_data(plan, span)?;
        let unique = self.strings;
        self.strings += 1;
        for (index, entry) in plan.entries.iter().enumerate() {
            self.codec_entry(unique, index, &entry.kind, span)?;
        }
        let table = format!("$json_codec_{unique}_{}", plan.root);
        self.codec_tables.insert(identity, table.clone());
        Ok(table)
    }
    /// Sum descriptors address real variant products, never fake tuple codec entries.
    fn codec_entry(
        &mut self,
        unique: usize,
        index: usize,
        kind: &Kind,
        span: Span,
    ) -> Lowering<()> {
        let prefix = format!("$json_codec_{unique}_{index}");
        if let Kind::Sum(variants) = kind {
            let mut rows = Vec::with_capacity(variants.len());
            for (tag, variant) in variants.iter().enumerate() {
                let children =
                    self.codec_children(unique, &format!("{prefix}_v{tag}"), &variant.fields);
                let name = self.string(&variant.wire_tag, span)?;
                rows.extend([
                    DataValue::Word(native_operand(&name)),
                    DataValue::Word(Operand::Int(variant.fields.len() as i64)),
                    DataValue::Word(native_operand(&children)),
                ]);
            }
            self.data.data(&format!("{prefix}_variants"), rows);
            self.data.data(
                &(prefix).to_string(),
                vec![
                    DataValue::Word(native_operand("12")),
                    DataValue::Word(native_operand(&(variants.len()).to_string())),
                    DataValue::Word(native_operand(&format!("{}_variants", prefix))),
                    DataValue::Word(native_operand("0")),
                ],
            );
            return Ok(());
        }
        let (tag, children) = descriptor(kind);
        let child_data = self.codec_children(unique, &prefix, &children);
        let mut names = Vec::new();
        if let Kind::Record(fields) = kind {
            for field in fields {
                names.push(DataValue::Word(native_operand(
                    &self.string(&field.name, span)?,
                )));
            }
        }
        let names = if names.is_empty() {
            "0".into()
        } else {
            self.data.data(&format!("{prefix}_names"), names);
            format!("{prefix}_names")
        };
        self.data.data(
            &(prefix).to_string(),
            vec![
                DataValue::Word(native_operand(&(tag).to_string())),
                DataValue::Word(native_operand(&(children.len()).to_string())),
                DataValue::Word(native_operand(&(child_data))),
                DataValue::Word(native_operand(&(names))),
            ],
        );
        Ok(())
    }
    /// Emit a bounded descriptor pointer array after aggregate output reservation.
    fn codec_children(&mut self, unique: usize, prefix: &str, ids: &[usize]) -> String {
        if ids.is_empty() {
            return "0".into();
        }
        let refs = ids
            .iter()
            .map(|id| DataValue::Word(native_operand(&format!("$json_codec_{unique}_{id}"))))
            .collect();
        self.data.data(&format!("{prefix}_children"), refs);
        format!("{prefix}_children")
    }
}
/// Match the documented runtime ABI without relying on Rust enum discriminant layout.
fn descriptor(kind: &Kind) -> (usize, Vec<usize>) {
    match kind {
        Kind::Sum(_) => unreachable!("sum descriptors use their typed variant table"),
        Kind::Union(ids) => (13, ids.clone()),
        Kind::Int => (0, vec![]),
        Kind::Float => (1, vec![]),
        Kind::Bool => (2, vec![]),
        Kind::String => (3, vec![]),
        Kind::Unit => (4, vec![]),
        Kind::Dynamic => (5, vec![]),
        Kind::Newtype(id) => (11, vec![*id]),
        Kind::List(id) => (6, vec![*id]),
        Kind::Option(id) => (7, vec![*id]),
        Kind::Tuple(ids) => (8, ids.clone()),
        Kind::Map(id) => (9, vec![*id]),
        Kind::Record(fields) => (10, fields.iter().map(|f| f.codec).collect()),
    }
}
