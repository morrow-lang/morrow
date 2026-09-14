//! Concrete nominal layouts and guarded recursive matching at the backend boundary.
use super::*;
#[path = "pattern_coverage.rs"]
mod coverage;
#[path = "sequence_patterns.rs"]
mod sequences;
#[path = "union_patterns.rs"]
mod union_patterns;

/// Index concrete named layouts, validating record metadata and referenced field types.
pub(super) fn layouts(types: &[ir::TypeLayout]) -> Lowering<HashMap<Type, &ir::TypeLayout>> {
    let mut layouts = HashMap::new();
    if types.len() > MAX_NODES {
        return Err(invalid(Span::default(), "nominal layout limit exceeded"));
    }
    for layout in types {
        concrete(&layout.ty, Span::default(), 0)?;
        if !matches!(layout.ty, Type::Named(_, _)) || layout.variants.is_empty() {
            return Err(invalid(
                Span::default(),
                "nominal layout requires a named type and variants",
            ));
        }
        if layout.storage == ir::LayoutStorage::Unboxed {
            newtypes::payload(layout, Span::default())?;
        }
        if layouts.insert(layout.ty.clone(), layout).is_some() {
            return Err(invalid(Span::default(), "duplicate nominal layout"));
        }
        if !layout.fields.is_empty()
            && (layout.variants.len() != 1 || layout.fields.len() != layout.variants[0].len())
        {
            return Err(invalid(
                Span::default(),
                "record field metadata differs from layout",
            ));
        }
        variant_names(layout)?;
        let unique: BTreeSet<_> = layout.fields.iter().collect();
        if unique.len() != layout.fields.len() {
            return Err(invalid(Span::default(), "duplicate record field names"));
        }
    }
    for layout in types {
        for fields in &layout.variants {
            if fields.len() > MAX_NODES {
                return Err(invalid(Span::default(), "nominal field limit exceeded"));
            }
            for field in fields {
                resolved(field, &layouts, Span::default(), 0)?;
            }
        }
        resolved(&layout.ty, &layouts, Span::default(), 0)?;
        newtypes::representation(&layout.ty, &layouts, Span::default())?;
    }
    Ok(layouts)
}

/// Names are inert source metadata, but malformed inactive layouts still cannot carry arbitrary tags.
fn variant_names(layout: &ir::TypeLayout) -> Lowering<()> {
    if layout.variant_names.is_empty() {
        return Ok(());
    }
    if layout.storage != ir::LayoutStorage::Tagged
        || !layout.fields.is_empty()
        || layout.variant_names.len() != layout.variants.len()
        || layout.variant_names.len() > 255
    {
        return Err(invalid(
            Span::default(),
            "invalid source constructor metadata",
        ));
    }
    let mut names = BTreeSet::new();
    for name in &layout.variant_names {
        if !crate::json_codec::sums::source_name(name) || !names.insert(name) {
            return Err(invalid(Span::default(), "invalid source constructor name"));
        }
    }
    Ok(())
}

/// Require every nominal reference to have an exact concrete layout without expanding cycles.
pub(super) fn resolved(
    ty: &Type,
    layouts: &HashMap<Type, &ir::TypeLayout>,
    span: Span,
    depth: usize,
) -> Lowering<()> {
    // Validate structural depth before hashing a recursively structured nominal key.
    concrete(ty, span, depth)?;
    match ty {
        Type::Never | Type::Infer(_) | Type::Generic(_) => {
            return Err(invalid(
                span,
                "unresolved inference variable or generic type",
            ));
        }
        Type::Named(_, args) => {
            if !layouts.contains_key(ty) {
                return Err(invalid(span, "missing concrete nominal layout"));
            }
            for arg in args {
                resolved(arg, layouts, span, depth + 1)?;
            }
        }
        Type::Function(args, result) => {
            for arg in args {
                resolved(arg, layouts, span, depth + 1)?;
            }
            resolved(result, layouts, span, depth + 1)?;
        }
        Type::Tuple(fields) | Type::Union(fields) => {
            for field in fields {
                resolved(field, layouts, span, depth + 1)?;
            }
        }
        Type::Pid(item) | Type::List(item) | Type::Option(item) => {
            resolved(item, layouts, span, depth + 1)?
        }
        Type::Map(key, value) => {
            maps::types(ty, layouts, span)?;
            resolved(key, layouts, span, depth + 1)?;
            resolved(value, layouts, span, depth + 1)?;
        }
        Type::ActorFunction(ok, err) | Type::Result(ok, err) => {
            resolved(ok, layouts, span, depth + 1)?;
            resolved(err, layouts, span, depth + 1)?;
        }
        Type::Range
        | Type::Native(_)
        | Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Unit => {}
    }
    Ok(())
}

impl Emitter<'_> {
    /// Validate and allocate one concrete variant, storing each checked field as 64 bits.
    pub(super) fn custom_construct(
        &mut self,
        tag: usize,
        fields: &[Expr],
        ty: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let expected = self.variant_fields(ty, tag, span)?;
        if !matches!(ty, Type::Named(_, _) | Type::Tuple(_)) || expected.len() != fields.len() {
            return Err(invalid(
                span,
                "custom constructor field count or nominal type mismatch",
            ));
        }
        let bytes = (fields.len() + 1)
            .checked_mul(8)
            .ok_or_else(|| invalid(span, "allocation size overflow"))?;
        let value = self.assign(
            locals,
            ty.clone(),
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand(&(bytes).to_string()))],
                variadic: None,
            },
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(tag).to_string()),
            address: native_operand(&(value)),
        });
        for (index, (field, expected)) in fields.iter().zip(expected).enumerate() {
            expect_type(field.ty.clone(), expected, field.span)?;
            let payload = self.expr(field, locals, depth)?;
            let payload = self.payload(locals, &field.ty, payload);
            let address = self.assign(
                locals,
                Type::Int,
                NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand(&(value).to_string()),
                    native_operand(&(8 * (index + 1)).to_string()),
                ),
            );
            self.output.statement(Statement::Store {
                kind: LoadKind::I64,
                value: native_operand(&(payload)),
                address: native_operand(&(address)),
            });
        }
        Ok((ty.clone(), value))
    }

    /// Select an indexed field only from a validated record with a single layout.
    pub(super) fn record_field(
        &mut self,
        value: &Expr,
        index: usize,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        if let Type::Tuple(fields) = &value.ty {
            let ty = fields
                .get(index)
                .cloned()
                .ok_or_else(|| invalid(value.span, "invalid tuple field index"))?;
            let record = self.expr(value, locals, depth)?;
            let payload = self.custom_field(&record, index, &ty, locals);
            return Ok((ty, payload));
        }
        let layout = self
            .layouts
            .get(&value.ty)
            .ok_or_else(|| invalid(value.span, "field access requires a nominal record"))?;
        if layout.fields.is_empty() || layout.variants.len() != 1 || index >= layout.fields.len() {
            return Err(invalid(value.span, "invalid record field index"));
        }
        let ty = layout.variants[0][index].clone();
        let record = self.expr(value, locals, depth)?;
        let payload = self.custom_field(&record, index, &ty, locals);
        Ok((ty, payload))
    }

    /// Read a custom field after its owner/tag has been checked by the caller.
    pub(super) fn custom_field(
        &mut self,
        value: &str,
        index: usize,
        ty: &Type,
        locals: &mut Locals,
    ) -> String {
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(value),
                native_operand(&(8 * (index + 1)).to_string()),
            ),
        );
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(address))),
        );
        self.unpack(locals, ty, raw)
    }

    /// Return a concrete variant's field types for builtin or nominal sums.
    fn variant_fields(&self, ty: &Type, tag: usize, span: Span) -> Lowering<Vec<Type>> {
        match (ty, tag) {
            (Type::Tuple(fields), 0) => Ok(fields.clone()),
            (Type::Option(item), 0) => Ok(vec![*item.clone()]),
            (Type::Option(_), 1) => Ok(vec![]),
            (Type::Result(ok, _), 0) => Ok(vec![*ok.clone()]),
            (Type::Result(_, err), 1) => Ok(vec![*err.clone()]),
            (Type::Named(_, _), _) => self
                .layouts
                .get(ty)
                .filter(|layout| layout.storage == ir::LayoutStorage::Tagged)
                .and_then(|layout| layout.variants.get(tag))
                .cloned()
                .ok_or_else(|| invalid(span, "unknown nominal type or variant tag")),
            _ => Err(invalid(
                span,
                "variant tag does not belong to the checked type",
            )),
        }
    }

    /// Normalize Unit or a structural tuple while checking each nested field once.
    fn checked_tuple_pattern(
        &self,
        fields: &[Pattern],
        ty: &Type,
        span: Span,
        depth: usize,
    ) -> Lowering<Pattern> {
        if fields.is_empty() {
            expect_type(ty.clone(), Type::Unit, span)?;
            return Ok(Pattern::Wildcard);
        }
        let Type::Tuple(types) = ty else {
            return Err(invalid(span, "tuple pattern requires tuple subject"));
        };
        if fields.len() != types.len() {
            return Err(invalid(span, "tuple pattern arity mismatch"));
        }
        let fields = fields
            .iter()
            .zip(types)
            .map(|(pattern, ty)| self.checked_pattern(pattern, ty, span, depth + 1))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Pattern::Variant { tag: 0, fields })
    }

    /// Normalize legacy constructors and recursively validate pattern arity and scalar types.
    fn checked_pattern(
        &self,
        pattern: &Pattern,
        ty: &Type,
        span: Span,
        depth: usize,
    ) -> Lowering<Pattern> {
        if depth > MAX_DEPTH {
            return Err(invalid(span, "pattern nesting limit exceeded"));
        }
        match pattern {
            Pattern::UnionSelect { narrowed, binding } => {
                self.checked_union_pattern(ty, narrowed, binding.as_ref(), span)
            }
            Pattern::Newtype(inner) => {
                let payload = self.newtype_payload(ty, span)?;
                Ok(Pattern::Newtype(Box::new(self.checked_pattern(
                    inner,
                    &payload,
                    span,
                    depth + 1,
                )?)))
            }
            Pattern::Tuple(fields) => self.checked_tuple_pattern(fields, ty, span, depth),
            Pattern::List { prefix, rest } => {
                self.checked_list_pattern(prefix, rest.as_deref(), ty, span, depth)
            }
            Pattern::TupleRest { prefix, rest } => {
                self.checked_tuple_rest(prefix, rest, ty, span, depth)
            }
            Pattern::Wildcard | Pattern::Bind(_) => Ok(pattern.clone()),
            Pattern::Int(_) => {
                expect_type(ty.clone(), Type::Int, span)?;
                Ok(pattern.clone())
            }
            Pattern::Bool(_) => {
                expect_type(ty.clone(), Type::Bool, span)?;
                Ok(pattern.clone())
            }
            Pattern::String(value) => {
                expect_type(ty.clone(), Type::String, span)?;
                if value.as_bytes().contains(&0) {
                    return Err(invalid(span, "NUL in string pattern"));
                }
                Ok(pattern.clone())
            }
            Pattern::Constructor {
                constructor,
                binding,
            } => checked_constructor_pattern(*constructor, *binding, ty, span),
            Pattern::Variant { tag, fields } => {
                let expected = self.variant_fields(ty, *tag, span)?;
                if expected.len() != fields.len() {
                    return Err(invalid(
                        span,
                        "pattern field count differs from variant layout",
                    ));
                }
                let fields = fields
                    .iter()
                    .zip(expected)
                    .map(|(field, ty)| self.checked_pattern(field, &ty, span, depth + 1))
                    .collect::<Result<_, _>>()?;
                Ok(Pattern::Variant { tag: *tag, fields })
            }
        }
    }
}

/// Normalize a legacy built-in constructor, rejecting bindings on payload-free None.
fn checked_constructor_pattern(
    constructor: Constructor,
    binding: Option<ir::LocalId>,
    ty: &Type,
    span: Span,
) -> Lowering<Pattern> {
    let payload = payload_type(constructor, ty, span)?;
    if payload.is_none() && binding.is_some() {
        return Err(invalid(span, "None pattern cannot bind a payload"));
    }
    let tag = usize::from(matches!(constructor, Constructor::None | Constructor::Err));
    let fields = if payload.is_some() {
        vec![binding.map_or(Pattern::Wildcard, Pattern::Bind)]
    } else {
        vec![]
    };
    Ok(Pattern::Variant { tag, fields })
}

/// Per-arm state tracks bindings that must disappear before any following arm.
struct PatternState<'a> {
    failure: &'a str,
    bindings: Vec<usize>,
    pending: Vec<sequences::Pending>,
    union_pending: Vec<union_patterns::Pending>,
    span: Span,
    depth: usize,
}

impl Emitter<'_> {
    /// Normalize match patterns and prove coverage using only unguarded arms.
    fn checked_match(
        &self,
        value: &Expr,
        arms: &[MatchArm],
        partial: bool,
    ) -> Lowering<(Type, Vec<Pattern>)> {
        arms.first()
            .ok_or_else(|| invalid(value.span, "match requires arms"))?;
        let mut result = Type::Never;
        let mut patterns = Vec::new();
        let mut covering = Vec::new();
        let mut budget = 8192;
        for arm in arms {
            if arm.body.ty != Type::Never {
                resolved(&arm.body.ty, &self.layouts, arm.span, 0)?;
            }
            result = control::joined(&result, &arm.body.ty, arm.span)?;
            if self.exhaustive(std::slice::from_ref(&value.ty), &covering, &mut budget, 0)? {
                return Err(invalid(arm.span, "unreachable match arm"));
            }
            let pattern = self.checked_pattern(&arm.pattern, &value.ty, arm.span, 0)?;
            if self.covered_candidate(
                std::slice::from_ref(&value.ty),
                &covering,
                std::slice::from_ref(&pattern),
                &mut budget,
                0,
            )? {
                return Err(invalid(arm.span, "unreachable duplicate match pattern"));
            }
            if let Some(guard) = &arm.guard {
                expect_type(guard.ty.clone(), Type::Bool, guard.span)?;
            } else {
                covering.push(vec![pattern.clone()]);
            }
            patterns.push(pattern);
        }
        if !partial
            && !self.exhaustive(std::slice::from_ref(&value.ty), &covering, &mut budget, 0)?
        {
            return Err(invalid(value.span, "nonexhaustive match"));
        }
        Ok((result, patterns))
    }

    /// Decide exhaustive coverage of a bounded pattern matrix, including nested products.
    fn exhaustive(
        &self,
        types: &[Type],
        rows: &[Vec<Pattern>],
        budget: &mut usize,
        depth: usize,
    ) -> Lowering<bool> {
        if *budget == 0 || depth > MAX_DEPTH {
            return Err(invalid(Span::default(), "match coverage limit exceeded"));
        }
        *budget -= 1;
        if rows.is_empty() {
            return Ok(false);
        }
        if coverage::complete_row(rows, types.len(), budget)? {
            return Ok(true);
        }
        let common = coverage::shared_columns(rows, types.len(), budget)?;
        if common > 0 {
            let rows = coverage::trim_columns(rows, common, budget)?;
            return self.exhaustive(&types[common..], &rows, budget, depth);
        }
        if let Some(variants) = self.coverage_variants(&types[0]) {
            for (tag, fields) in variants.into_iter().enumerate() {
                let mut specialized = Vec::new();
                for row in rows {
                    if let Some(mut head) = specialize(&row[0], &types[0], tag, fields.len()) {
                        head.extend_from_slice(&row[1..]);
                        specialized.push(head);
                    }
                }
                let mut expanded = fields;
                expanded.extend_from_slice(&types[1..]);
                if !self.exhaustive(&expanded, &specialized, budget, depth + 1)? {
                    return Ok(false);
                }
            }
            Ok(true)
        } else {
            let default: Vec<_> = rows
                .iter()
                .filter(|row| catchall(&row[0]))
                .map(|row| row[1..].to_vec())
                .collect();
            self.exhaustive(&types[1..], &default, budget, depth + 1)
        }
    }

    /// Enumerate finite constructors, modeling array lists as logical empty/cons values.
    fn coverage_variants(&self, ty: &Type) -> Option<Vec<Vec<Type>>> {
        match ty {
            Type::Union(members) => Some(vec![vec![]; members.len()]),
            Type::Bool => Some(vec![vec![], vec![]]),
            Type::Unit => Some(vec![vec![]]),
            Type::Option(item) => Some(vec![vec![*item.clone()], vec![]]),
            Type::Result(ok, err) => Some(vec![vec![*ok.clone()], vec![*err.clone()]]),
            Type::Tuple(fields) => Some(vec![fields.clone()]),
            Type::List(item) => Some(vec![vec![], vec![*item.clone(), ty.clone()]]),
            Type::Named(_, _) => self.layouts.get(ty).map(|layout| layout.variants.clone()),
            _ => None,
        }
    }

    /// Match once, enter guards only after bindings exist, and merge actual arm endpoints.
    pub(super) fn matching(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<(Type, String)> {
        self.matching_mode(value, arms, locals, depth, tail, false)
    }

    /// Partial compiler selectors return zero only after every arm fails.
    pub(super) fn matching_mode(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        locals: &mut Locals,
        depth: usize,
        tail: bool,
        partial: bool,
    ) -> Lowering<(Type, String)> {
        let (ty, patterns) = self.checked_match(value, arms, partial)?;
        let scrutinee = self.expr(value, locals, depth)?;
        let merge = locals.label();
        let mut incoming = Vec::new();
        for (arm, pattern) in arms.iter().zip(patterns) {
            let failure = locals.label();
            let mut state = PatternState {
                failure: &failure,
                bindings: vec![],
                pending: vec![],
                union_pending: vec![],
                span: arm.span,
                depth,
            };
            self.pattern_branch(&pattern, &value.ty, &scrutinee, &mut state, locals)?;
            self.materialize_rests(&mut state, locals)?;
            let result = self.match_arm(arm, &failure, locals, depth, tail);
            for id in state.bindings {
                locals.values.remove(&id);
            }
            self.incoming(result, &mut incoming, &merge, locals)?;
            self.start_block(locals, &failure);
        }
        // Ordinary matches are exhaustive; selector failure preserves the unmatched message.
        if partial {
            self.incoming(Ok("0".into()), &mut incoming, &merge, locals)?;
        } else {
            self.output.statement(Statement::Trap);
        }
        self.join(ty, incoming, &merge, locals)
    }

    /// Guard/body termination ends only this arm, preserving later pattern-failure paths.
    fn match_arm(
        &mut self,
        arm: &MatchArm,
        failure: &str,
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<String> {
        if let Some(guard) = &arm.guard {
            let test = self.expr(guard, locals, depth)?;
            self.require_pattern(&test, failure, locals);
        }
        self.position_expr(&arm.body, locals, depth, tail)
    }

    /// Bind a successful pattern in the surrounding block; failure must leave the function.
    pub(super) fn let_else(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        otherwise: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<()> {
        if otherwise.ty != Type::Never {
            return Err(invalid(otherwise.span, "let-else failure must terminate"));
        }
        let scrutinee = self.expr(value, locals, depth)?;
        let pattern = self.checked_pattern(pattern, &value.ty, value.span, 0)?;
        let outer = locals.values.clone();
        let failure = locals.label();
        let success = locals.label();
        let mut state = PatternState {
            failure: &failure,
            bindings: vec![],
            pending: vec![],
            union_pending: vec![],
            span: value.span,
            depth,
        };
        self.pattern_branch(&pattern, &value.ty, &scrutinee, &mut state, locals)?;
        self.materialize_rests(&mut state, locals)?;
        let bound = locals.values.clone();
        self.output
            .statement(Statement::Jump((success).to_string()));
        locals.values = outer;
        self.start_block(locals, &failure);
        match self.expr(otherwise, locals, depth) {
            Err(Exit::Terminated) => {}
            Err(error) => return Err(error),
            Ok(_) => return Err(invalid(otherwise.span, "let-else failure continued")),
        }
        locals.values = bound;
        self.start_block(locals, &success);
        Ok(())
    }

    /// Branch to the next arm on failure, preserving a fresh successful predecessor.
    fn require_pattern(&mut self, test: &str, failure: &str, locals: &mut Locals) {
        let success = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(test),
            then_label: (success).to_string(),
            else_label: (failure).to_string(),
        });
        self.start_block(locals, &success);
    }

    /// Check nested tags before payload reads, introducing scoped SSA bindings as found.
    fn pattern_branch(
        &mut self,
        pattern: &Pattern,
        ty: &Type,
        value: &str,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        self.nodes += 1;
        if self.nodes > MAX_NODES || state.depth > MAX_DEPTH {
            return Err(invalid(state.span, "pattern lowering limit exceeded"));
        }
        match pattern {
            Pattern::UnionSelect { narrowed, binding } => {
                self.union_pattern(ty, narrowed, binding.as_ref(), value, state, locals)?;
            }
            Pattern::List { prefix, rest } => {
                self.list_pattern(prefix, rest.as_deref(), ty, value, state, locals)?
            }
            Pattern::TupleRest { prefix, rest } => {
                self.tuple_rest_pattern(prefix, rest, ty, value, state, locals)?
            }
            Pattern::Newtype(inner) => {
                let payload = self.newtype_payload(ty, state.span)?;
                state.depth += 1;
                self.pattern_branch(inner, &payload, value, state, locals)?;
                state.depth -= 1;
            }
            Pattern::Wildcard => {}
            Pattern::Bind(id) => {
                locals.define(id.0, ty.clone(), value.to_owned(), state.span)?;
                state.bindings.push(id.0);
            }
            Pattern::Variant { tag, fields } => {
                let test = self.variant_test(ty, *tag, value, locals);
                self.require_pattern(&test, state.failure, locals);
                let field_types = self.variant_fields(ty, *tag, state.span)?;
                for (index, (pattern, ty_field)) in fields.iter().zip(field_types).enumerate() {
                    if matches!(pattern, Pattern::Wildcard) {
                        continue;
                    }
                    let field = self.variant_field(ty, &ty_field, value, index, locals);
                    state.depth += 1;
                    self.pattern_branch(pattern, &ty_field, &field, state, locals)?;
                    state.depth -= 1;
                }
            }
            _ => {
                let test = self.scalar_test(pattern, value, state.span, locals)?;
                self.require_pattern(&test, state.failure, locals);
            }
        }
        Ok(())
    }

    /// Test a concrete custom discriminant or the shared heap Result tag convention.
    fn variant_test(&mut self, ty: &Type, tag: usize, value: &str, locals: &mut Locals) -> String {
        if matches!(ty, Type::Named(_, _) | Type::Tuple(_)) {
            let actual = self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(value)),
            );
            self.assign(
                locals,
                Type::Bool,
                NativeOperation::Binary(
                    MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                    native_operand(&(actual)),
                    native_operand(&(tag).to_string()),
                ),
            )
        } else {
            let is_ok = self.assign(
                locals,
                Type::Bool,
                NativeOperation::Call {
                    callee: native_operand("$fern_result_is_ok"),
                    args: vec![(Scalar::I64, native_operand(value))],
                    variadic: None,
                },
            );
            if tag == 0 {
                is_ok
            } else {
                self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Binary(
                        MachineBinary::Compare(Comparison::Eq, Scalar::I32),
                        native_operand(&(is_ok)),
                        native_operand("0"),
                    ),
                )
            }
        }
    }

    /// Read a selected variant field only after its tag succeeded in the current block.
    fn variant_field(
        &mut self,
        owner: &Type,
        ty: &Type,
        value: &str,
        index: usize,
        locals: &mut Locals,
    ) -> String {
        if matches!(owner, Type::Named(_, _) | Type::Tuple(_)) {
            self.custom_field(value, index, ty, locals)
        } else {
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_result_unwrap"),
                    args: vec![(Scalar::I64, native_operand(value))],
                    variadic: None,
                },
            );
            self.unpack(locals, ty, raw)
        }
    }

    /// Compare validated scalar literals; other patterns must use dedicated lowering.
    fn scalar_test(
        &mut self,
        pattern: &Pattern,
        value: &str,
        span: Span,
        locals: &mut Locals,
    ) -> Lowering<String> {
        let instruction = match pattern {
            Pattern::Int(integer) => NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                native_operand(value),
                native_operand(&(integer).to_string()),
            ),
            Pattern::Bool(boolean) => NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Eq, Scalar::I32),
                native_operand(value),
                native_operand(&(u8::from(*boolean)).to_string()),
            ),
            Pattern::String(text) => {
                let text = self.string(text, span)?;
                NativeOperation::Call {
                    callee: native_operand("$fern_str_eq"),
                    args: vec![
                        (Scalar::I64, native_operand(value)),
                        (Scalar::I64, native_operand(&(text))),
                    ],
                    variadic: None,
                }
            }
            _ => return Err(invalid(span, "unnormalized pattern in scalar lowering")),
        };
        Ok(self.assign(locals, Type::Bool, instruction))
    }
}

/// Recognize a pattern covering every value without inspecting its representation.
fn catchall(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::Wildcard | Pattern::Bind(_))
        || matches!(pattern, Pattern::List {prefix, rest: Some(_)} if prefix.is_empty())
}

/// Expand one matrix row into a chosen finite constructor, or discard mismatched rows.
fn specialize(pattern: &Pattern, ty: &Type, tag: usize, fields: usize) -> Option<Vec<Pattern>> {
    match pattern {
        Pattern::UnionSelect { narrowed, .. } => {
            let Type::Union(members) = ty else {
                return None;
            };
            members
                .get(tag)
                .filter(|member| crate::unions::members(narrowed).contains(member))
                .map(|_| vec![])
        }
        Pattern::List { prefix, rest } => sequences::specialize_list(prefix, rest.as_deref(), tag),
        Pattern::TupleRest { prefix, .. } if tag == 0 => {
            let mut expanded = prefix.clone();
            expanded.resize(fields, Pattern::Wildcard);
            Some(expanded)
        }
        Pattern::Newtype(inner) if tag == 0 && fields == 1 => Some(vec![*inner.clone()]),
        Pattern::Wildcard | Pattern::Bind(_) => Some(vec![Pattern::Wildcard; fields]),
        Pattern::Bool(value) if usize::from(*value) == tag => Some(vec![]),
        Pattern::Variant {
            tag: actual,
            fields,
        } if *actual == tag => Some(fields.clone()),
        _ => None,
    }
}

/// Recognize direct structural subsumption without treating guarded rows as coverage.
fn subsumes(prior: &Pattern, next: &Pattern) -> bool {
    match (prior, next) {
        (
            Pattern::List {
                prefix: a,
                rest: ar,
            },
            Pattern::List {
                prefix: b,
                rest: br,
            },
        ) => {
            (ar.is_some() || (br.is_none() && a.len() == b.len()))
                && a.len() <= b.len()
                && a.iter().zip(b).all(|(a, b)| subsumes(a, b))
        }
        (Pattern::UnionSelect { narrowed: a, .. }, Pattern::UnionSelect { narrowed: b, .. }) => {
            crate::unions::subset(b, a)
        }
        (Pattern::Newtype(a), Pattern::Newtype(b)) => subsumes(a, b),
        (Pattern::Wildcard | Pattern::Bind(_), _) => true,
        (Pattern::Int(a), Pattern::Int(b)) => a == b,
        (Pattern::Bool(a), Pattern::Bool(b)) => a == b,
        (Pattern::String(a), Pattern::String(b)) => a == b,
        (Pattern::Variant { tag: a, fields: af }, Pattern::Variant { tag: b, fields: bf }) => {
            a == b && af.len() == bf.len() && af.iter().zip(bf).all(|(a, b)| subsumes(a, b))
        }
        _ => false,
    }
}

impl Emitter<'_> {
    /// Bind only patterns proven to cover every value of their concrete input type.
    pub(super) fn bind_irrefutable(
        &mut self,
        pattern: &Pattern,
        ty: &Type,
        value: &str,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<()> {
        let pattern = self.checked_pattern(pattern, ty, span, 0)?;
        let mut budget = 8192;
        if !self.exhaustive(
            std::slice::from_ref(ty),
            &[vec![pattern.clone()]],
            &mut budget,
            0,
        )? {
            return Err(invalid(span, "binding pattern must be irrefutable"));
        }
        let failure = locals.label();
        let success = locals.label();
        let mut state = PatternState {
            failure: &failure,
            bindings: vec![],
            pending: vec![],
            union_pending: vec![],
            span,
            depth,
        };
        self.pattern_branch(&pattern, ty, value, &mut state, locals)?;
        self.materialize_rests(&mut state, locals)?;
        self.output
            .statement(Statement::Jump((success).to_string()));
        self.start_block(locals, &failure);
        self.output.statement(Statement::Trap);
        self.start_block(locals, &success);
        Ok(())
    }
}
