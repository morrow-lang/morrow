//! Contextual function typing, lexical capture discovery, and higher-order signatures.
use super::*;

impl Checker<'_> {
    /// Check a lambda in the enclosing inference graph while isolating its return context.
    pub(super) fn lambda(
        &mut self,
        params: &[ast::LambdaParam],
        body: &ast::Expr,
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let previous = std::mem::replace(&mut self.deferred, false);
        let result = self.lambda_body(params, body, expected, span, depth);
        self.deferred = previous;
        result
    }

    /// Share lambda construction while allowing the synthetic defer thunk its restricted context.
    pub(super) fn lambda_body(
        &mut self,
        params: &[ast::LambdaParam],
        body: &ast::Expr,
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let types = self.lambda_parameter_types(params, span)?;
        let result = self.inference.fresh();
        let ordinary = Type::Function(types.clone(), Box::new(result.clone()));
        let contextual = self.contextual_body(body);
        let mailbox = expected.and_then(|t| {
            if let Type::ActorFunction(m, _) = t {
                Some((**m).clone())
            } else {
                None
            }
        }).or_else(|| contextual.then(|| self.mailbox.clone()).flatten());
        let ty = mailbox
            .as_ref()
            .map(|m| Type::ActorFunction(Box::new(m.clone()), Box::new(ordinary.clone())))
            .unwrap_or_else(|| if contextual { Type::RootFunction(Box::new(ordinary.clone())) } else { ordinary });
        if let Some(expected) = expected {
            let expected = self.inference.resolve(expected, span)?;
            if matches!(expected, Type::Union(_)) {
                self.union_context(&ty, &expected, span)?;
            } else {
                self.inference.unify(&ty, &expected, span, "lambda type")?;
            }
        }
        let outer_count = self.local_count;
        self.scopes.push(HashMap::new());
        let params = params
            .iter()
            .zip(types)
            .map(|(param, ty)| ir::Param {
                id: self.bind_source(&param.name, ty.clone(), param.span),
                ty,
            })
            .collect();
        let previous = std::mem::replace(&mut self.function_return, result.clone());
        let previous_loop = std::mem::replace(&mut self.loop_depth, 0);
        let previous_mailbox = std::mem::replace(&mut self.mailbox, mailbox);
        let checked = self.expression_expected(body, Some(&result), depth);
        self.loop_depth = previous_loop;
        self.mailbox = previous_mailbox;
        self.function_return = previous;
        self.scopes.pop();
        let body = checked?;
        let captures = captures(&body, outer_count)?;
        Ok((
            ir::ExprKind::Lambda {
                params,
                captures,
                body: Box::new(body),
                local_count: self.local_count,
            },
            ty,
        ))
    }

    /// Validate lambda binders and their optional annotations before creating lexical locals.
    fn lambda_parameter_types(
        &mut self,
        params: &[ast::LambdaParam],
        span: Span,
    ) -> Checked<Vec<Type>> {
        let mut names = HashSet::new();
        if params.len() > MAX_PARAMETERS {
            return Err(Diagnostic::new(span, "lambda parameter limit exceeded"));
        }
        let mut types = Vec::new();
        for param in params {
            if !names.insert(&param.name) {
                return Err(Diagnostic::new(param.span, "duplicate lambda parameter"));
            }
            if let Some(ty) = &param.annotation {
                self.registry
                    .validate(ty, &self.inference.template_names, param.span)?;
            }
            types.push(
                param
                    .annotation
                    .clone()
                    .unwrap_or_else(|| self.inference.fresh()),
            );
        }
        Ok(types)
    }

    /// Push tuple annotation fields into contained function values before checking their bodies.
    pub(super) fn tuple(
        &mut self,
        values: &[ast::Expr],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        let fields = values
            .iter()
            .map(|_| self.inference.fresh())
            .collect::<Vec<_>>();
        let ty = Type::Tuple(fields.clone());
        self.constrain_result(&ty, expected, span)?;
        let values = values
            .iter()
            .zip(fields)
            .map(|(value, field)| self.expression_expected(value, Some(&field), depth))
            .collect::<Checked<Vec<_>>>()?;
        Ok((ir::ExprKind::Tuple(values), ty))
    }

    /// Apply a computed callable, evaluating its callee before ordered argument values.
    pub(super) fn apply(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if let ast::ExprKind::Field { value, name } = &callee.kind
            && name == "as_ptr"
        {
            let value = self.expression(value, depth)?;
            return self.foreign_string_borrow(value, args, span);
        }
        if let ast::ExprKind::Field { value, name } = &callee.kind
            && name == "enumerate"
        {
            let value = self.expression(value, depth)?;
            if value.ty == Type::Never {
                return Ok((value.kind, Type::Never));
            }
            if matches!(self.inference.resolve(&value.ty, span)?, Type::List(_)) {
                return self.enumerate_value(value, args, expected, span);
            }
            let (kind, ty) = self.field(value, name, callee.span)?;
            return self.invoke(
                ir::Expr {
                    kind,
                    ty,
                    span: callee.span,
                },
                args,
                expected,
                span,
                depth,
            );
        }
        let callee = self.expression(callee, depth)?;
        self.invoke(callee, args, expected, span, depth)
    }

    fn invoke(
        &mut self,
        callee: ir::Expr,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        labels::positional(args)?;
        if callee.ty == Type::Never {
            return Ok((callee.kind, Type::Never));
        }
        let ty = self.inference.resolve(&callee.ty, span)?;
        let (params, result) = match ty {
            Type::Function(params, result) => (params, *result),
            Type::RootFunction(function) => {
                if self.mailbox.is_some() { return Err(Diagnostic::new(span, "root function cannot run in an actor context")); }
                let Type::Function(params, result) = *function else { return Err(Diagnostic::new(span, "invalid root function signature")); };
                (params, *result)
            }
            Type::ActorFunction(mailbox, function) => {
                let owner = self.mailbox.as_ref().ok_or_else(|| Diagnostic::new(span, "actor function requires an actor context"))?;
                self.inference.unify(&mailbox, owner, span, "actor callable mailbox")?;
                let Type::Function(params, result) = *function else { return Err(Diagnostic::new(span, "invalid actor function signature")); };
                (params, *result)
            }
            Type::Infer(_) => {
                let params = args
                    .iter()
                    .map(|_| self.inference.fresh())
                    .collect::<Vec<_>>();
                let result = self.inference.fresh();
                self.inference.unify(
                    &callee.ty,
                    &Type::Function(params.clone(), Box::new(result.clone())),
                    span,
                    "callable type",
                )?;
                (params, result)
            }
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "value is not callable; expected a function",
                ));
            }
        };
        self.constrain_result(&result, expected, span)?;
        let args = self.call_arguments(args, &params, span, depth)?;
        Ok((
            ir::ExprKind::Invoke {
                callee: Box::new(callee),
                args,
            },
            result,
        ))
    }

    /// Resolve lexical callable values before global functions and type-directed constructors.
    pub(super) fn call_expected(
        &mut self,
        name: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if let Some(receiver) = name.strip_suffix(".as_ptr")
            && self
                .local(receiver.split('.').next().unwrap_or(receiver))
                .is_some()
        {
            let (kind, ty) = self.name(receiver, span)?;
            return self.foreign_string_borrow(ir::Expr { kind, ty, span }, args, span);
        }
        if let Some(receiver) = name.strip_suffix(".enumerate")
            && self
                .local(receiver.split('.').next().unwrap_or(receiver))
                .is_some()
        {
            let (_, ty) = self.name(receiver, span)?;
            if matches!(self.inference.resolve(&ty, span)?, Type::List(_)) {
                return self.enumerate_receiver(receiver, args, expected, span, depth);
            }
        }
        if self.local(name.split('.').next().unwrap_or(name)).is_some() {
            let (kind, ty) = self
                .name(name, span)
                .map_err(|e| context(e, "local binding shadows callable path"))?;
            return self.invoke(ir::Expr { kind, ty, span }, args, expected, span, depth);
        }
        self.callable_name(name, span)?;
        self.global_call(name, args, expected, span, depth)
    }

    /// Resolve a loader-proven global callee while checking arguments in the original scope.
    pub(super) fn global_call(
        &mut self,
        name: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        if self.deferred && self.signatures.get(name).is_some_and(|s| s.contextual) { return Err(Diagnostic::new(span, "supervisor helper cannot run in deferred cleanup")); }
        if self.constant_path(name) {
            let (kind, ty) = self.global_name(name, span)?;
            return self.invoke(ir::Expr { kind, ty, span }, args, expected, span, depth);
        }
        if crate::ffi::is_api(name) {
            return self.foreign_api(name, args, expected, span, depth);
        }
        if crate::supervisors::is_api(name) {
            return self.supervisor_call(name, args, expected, span, depth);
        }
        if crate::processes::is_api(name) {
            return self.process_call(name, args, expected, span, depth);
        }
        if sets::is_api(name) {
            return self.set_call(name, args, expected, span, depth);
        }
        if matches!(name, "spawn" | "send" | "supervise" | "supervised_current") {
            return self.actor_call(name, args, expected, span, depth);
        }
        if self
            .signatures
            .get(name)
            .is_some_and(|s| s.mailbox.is_some() || s.contextual && self.mailbox.is_some())
        {
            return self.actor_named_call(name, args, expected, span, depth);
        }
        if crate::codec_syntax::is_codec(name) {
            return self.json_codec(name, args, expected, span, depth);
        }
        if !self.signatures.contains_key(name) {
            labels::positional(args)?;
        }
        if self.registry.is_alias(name)
            && !self.signatures.contains_key(name)
            && self.registry.constructor(name).is_none()
        {
            return Err(Diagnostic::new(
                span,
                "a type alias does not introduce a value or constructor",
            ));
        }
        if self.registry.constructor(name).is_some() {
            return self.custom_construct(name, args, expected, span, depth);
        }
        if let Some(constructor) = constructor(name) {
            return self.construct(constructor, args, expected, span, depth);
        }
        let (target, params, result) = self.resolve_callable(name, span)?;
        if matches!(target, ir::CallTarget::Builtin(ir::Builtin::ListSortBy))
            && self.registry.constructor("Less").is_none()
        {
            // The comparator's `Ordering` result lives in the trait prelude.
            return Err(Diagnostic::new(span, super::ORD_PRELUDE_REQUIRED));
        }
        self.constrain_result(&result, expected, span)?;
        // A wrong argument type is the more useful report; a missing label waits until types agree.
        let (order, missing_label) = if let Some(signature) = self.signatures.get(name) {
            let order = labels::order(args, &signature.labels, span)?;
            let missing = labels::required(args, signature, &order).err();
            (order, missing)
        } else {
            ((0..args.len()).collect(), None)
        };
        let written = if self.signatures.contains_key(name) {
            order.iter().map(|index| params[*index].clone()).collect()
        } else {
            params
        };
        let mut args = self.call_arguments(args, &written, span, depth)?;
        if let Some(missing) = missing_label {
            return Err(missing);
        }
        if matches!(
            target,
            ir::CallTarget::Builtin(ir::Builtin::Print | ir::Builtin::Println)
        ) && let Some(value) = args.pop()
        {
            args.push(self.shown(value, span)?);
        }
        if let ir::CallTarget::Runtime(id) = target
            && runtime::signature(id)
                .is_some_and(|signature| signature.operation == runtime::Operation::ScalarSort)
            && let Some(sorted) = self.trait_sort(&mut args, span)?
        {
            return Ok(sorted);
        }
        Ok(self.ordered_call(target, args, &order, result, span))
    }

    /// `List.sort` over structured or generic elements sorts through the `Ord` method `compare`;
    /// scalar elements keep the runtime word/float/byte orders.
    fn trait_sort(&mut self, args: &mut Vec<ir::Expr>, span: Span) -> Checked<Option<TypedKind>> {
        let [list] = args.as_slice() else {
            return Ok(None);
        };
        let Type::List(item) = self.inference.resolve(&list.ty, list.span)? else {
            return Ok(None);
        };
        if !structured(&item) || self.inference.sorts_as_scalar(&item, list.span)? {
            return Ok(None);
        }
        let is_trait_method = self
            .registry
            .traits
            .methods
            .get("compare")
            .is_some_and(|(id, _)| self.registry.traits.name(*id) == Some("Ord"));
        if !is_trait_method || !self.signatures.contains_key("compare") {
            return Err(Diagnostic::new(list.span, super::ORD_PRELUDE_REQUIRED));
        }
        let (kind, ty) = self.global_name("compare", span)?;
        let compare = ir::Expr { kind, ty, span };
        let (params, result) = self.higher_order_signature(ir::Builtin::ListSortBy);
        let list = args.pop().expect("one argument");
        self.inference
            .unify(&list.ty, &params[0], list.span, "List.sort argument")?;
        self.inference
            .unify(&compare.ty, &params[1], span, "List.sort comparator")?;
        Ok(Some(self.ordered_call(
            ir::CallTarget::Builtin(ir::Builtin::ListSortBy),
            vec![list, compare],
            &[0, 1],
            result,
            span,
        )))
    }

    /// Print structured values through their `Show` implementation; scalars keep the direct path.
    /// Generic parameters also route through `show`, so templates require `Show(a)` of callers.
    fn shown(&mut self, value: ir::Expr, span: Span) -> Checked<ir::Expr> {
        let resolved = self.inference.resolve(&value.ty, value.span)?;
        if !structured(&resolved) {
            return Ok(value);
        }
        let is_trait_method = self
            .registry
            .traits
            .methods
            .get("show")
            .is_some_and(|(id, _)| self.registry.traits.name(*id) == Some("Show"));
        if !is_trait_method || !self.signatures.contains_key("show") {
            // A program-defined `show` shadows the prelude: generic parameters then keep the
            // direct print capability (scalar instantiations only) instead of failing outright.
            if matches!(resolved, Type::Generic(_)) && self.signatures.contains_key("show") {
                return Ok(value);
            }
            return Err(Diagnostic::new(value.span, super::SHOW_PRELUDE_REQUIRED));
        }
        let (target, params, result) = self.resolve_callable("show", span)?;
        let [param] = params.as_slice() else {
            return Err(Diagnostic::new(span, "show requires exactly one parameter"));
        };
        self.inference
            .unify(&value.ty, param, value.span, "print argument")?;
        let value_span = value.span;
        let (kind, ty) = self.ordered_call(target, vec![value], &[0], result, span);
        Ok(ir::Expr {
            kind,
            ty,
            span: value_span,
        })
    }

    /// Feed compatible result context into arguments; report outer shape errors after argument errors.
    pub(super) fn constrain_result(
        &mut self,
        result: &Type,
        expected: Option<&Type>,
        span: Span,
    ) -> Checked<()> {
        if let Some(expected) = expected {
            if matches!(self.inference.resolve(expected, span)?, Type::Infer(_)) {
                return Ok(());
            }
            let actual = self.inference.resolve(result, span)?;
            let expected = self.inference.resolve(expected, span)?;
            if matches!(actual, Type::Union(_)) {
                return Ok(());
            }
            if matches!(expected, Type::Union(_)) {
                return self.union_context(&actual, &expected, span);
            }
            if matches!(actual, Type::Infer(_))
                || matches!(expected, Type::Infer(_))
                || std::mem::discriminant(&actual) == std::mem::discriminant(&expected)
            {
                self.inference
                    .unify(&actual, &expected, span, "call result")?;
            }
        }
        Ok(())
    }

    /// Gather ordinary argument constraints before lambdas while retaining source evaluation order.
    pub(super) fn call_arguments(
        &mut self,
        args: &[ast::Argument],
        params: &[Type],
        span: Span,
        depth: usize,
    ) -> Checked<Vec<ir::Expr>> {
        if args.len() != params.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "function expects {} argument(s), found {}",
                    params.len(),
                    args.len()
                ),
            ));
        }
        let exact: Vec<_> = params
            .iter()
            .map(|param| {
                Ok(matches!(
                    self.inference.resolve(param, span)?,
                    Type::Infer(_)
                ))
            })
            .collect::<Checked<_>>()?;
        let delayed: Vec<_> = args.iter().map(|arg| contains_lambda(arg)).collect();
        let order = params
            .iter()
            .zip(&delayed)
            .map(|(param, delayed)| {
                let union = matches!(self.inference.resolve(param, span)?, Type::Union(_));
                Ok(if *delayed { 2 } else { usize::from(union) })
            })
            .collect::<Checked<Vec<_>>>()?;
        let mut checked = vec![None; args.len()];
        for phase in 0..3 {
            for (index, (arg, param)) in args.iter().zip(params).enumerate() {
                if order[index] == phase {
                    checked[index] = Some(
                        if exact[index] {
                            self.expression_equal(arg, param, depth)
                        } else {
                            self.expression_expected(arg, Some(param), depth)
                        }
                        .map_err(|e| context(e, "call argument"))?,
                    );
                }
            }
        }
        Ok(checked.into_iter().map(Option::unwrap).collect())
    }

    /// Normalize each lambda independently; delayed reads do not handle outer Result values.
    pub(super) fn finalize_lambda(
        &self,
        params: &mut [ir::Param],
        captures: &mut [ir::Capture],
        body: &mut ir::Expr,
    ) -> Checked<()> {
        for param in params.iter_mut() {
            param.ty = self.inference.concrete(&param.ty, body.span)?;
        }
        for capture in captures.iter_mut() {
            self.finalize(&mut capture.value)?;
            capture.param.ty = capture.value.ty.clone();
            if self.registry.contains_result(&capture.param.ty)? {
                return Err(Diagnostic::new(
                    capture.value.span,
                    "capturing Result-bearing values in closures is currently unsupported; handle the Result before capturing",
                ));
            }
        }
        self.finalize(body)?;
        reject_unused_results(body, params, self.registry)
    }

    /// Validate intrinsic constraints using a fully concrete first-class function signature.
    pub(super) fn validate_function_value(
        &self,
        target: ir::CallTarget,
        ty: &Type,
        span: Span,
    ) -> Checked<()> {
        let Some((mailbox, params, result)) = crate::actors::function(ty) else {
            return Err(Diagnostic::new(span, "invalid function value type"));
        };
        if let Some(mailbox) = mailbox {
            return self.actor_requirements(target, params, result, mailbox, span);
        }
        let args = params
            .iter()
            .enumerate()
            .map(|(id, ty)| ir::Expr {
                kind: ir::ExprKind::Local(ir::LocalId(id)),
                ty: ty.clone(),
                span,
            })
            .collect::<Vec<_>>();
        self.named_requirements(target, params, result, span)?;
        validate_builtin(target, &args, span, &self.inference)
    }

    /// Instantiate independent payload, accumulator, and error variables for each combinator.
    pub(super) fn higher_order_signature(&mut self, builtin: ir::Builtin) -> (Vec<Type>, Type) {
        use ir::Builtin::*;
        let a = self.inference.fresh();
        let b = self.inference.fresh();
        let e = self.inference.fresh();
        let list = Type::List(Box::new(a.clone()));
        let result = Type::Result(Box::new(a.clone()), Box::new(e.clone()));
        let callback = |params, result| Type::Function(params, Box::new(result));
        match builtin {
            ListMap => (
                vec![list, callback(vec![a], b.clone())],
                Type::List(Box::new(b)),
            ),
            ListSortBy => (
                vec![
                    list.clone(),
                    callback(
                        vec![a.clone(), a],
                        Type::Named("Ordering".to_owned(), vec![]),
                    ),
                ],
                list,
            ),
            ListFold => (
                vec![list, b.clone(), callback(vec![b.clone(), a], b.clone())],
                b,
            ),
            ListFilter => (vec![list.clone(), callback(vec![a], Type::Bool)], list),
            ListFind => (
                vec![list, callback(vec![a.clone()], Type::Bool)],
                Type::Option(Box::new(a)),
            ),
            ListAny | ListAll => (vec![list, callback(vec![a], Type::Bool)], Type::Bool),
            OptionMap => (
                vec![
                    Type::Option(Box::new(a.clone())),
                    callback(vec![a], b.clone()),
                ],
                Type::Option(Box::new(b)),
            ),
            ResultMap => (
                vec![result, callback(vec![a], b.clone())],
                Type::Result(Box::new(b), Box::new(e)),
            ),
            ResultAndThen => {
                let output = Type::Result(Box::new(b), Box::new(e));
                (vec![result, callback(vec![a], output.clone())], output)
            }
            ResultUnwrapOrElse => (vec![result, callback(vec![e], a.clone())], a),
            _ => unreachable!("higher-order signature called only for combinators"),
        }
    }
}

/// Types whose printing and ordering dispatch through derived trait implementations.
fn structured(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(_)
            | Type::Map(_, _)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::Tuple(_)
            | Type::Named(_, _)
            | Type::Union(_)
            | Type::Generic(_)
    )
}

/// Capture original lexical IDs deterministically; nested lambdas expose transitive free values.
fn captures(body: &ir::Expr, outer_count: usize) -> Checked<Vec<ir::Capture>> {
    let mut found = std::collections::BTreeMap::new();
    let mut pending = vec![body];
    while let Some(expr) = pending.pop() {
        if let ir::ExprKind::Local(id) = expr.kind
            && id.0 < outer_count
        {
            found.entry(id.0).or_insert_with(|| expr.clone());
        }
        pending.extend(nominal::children(expr));
    }
    if found.len() > MAX_PARAMETERS {
        return Err(Diagnostic::new(
            body.span,
            "closure capture limit exceeded (255)",
        ));
    }
    Ok(found
        .into_values()
        .map(|value| {
            let ir::ExprKind::Local(id) = value.kind else {
                unreachable!()
            };
            ir::Capture {
                param: ir::Param {
                    id,
                    ty: value.ty.clone(),
                },
                value,
            }
        })
        .collect())
}

/// Discover deferred lambda-containing arguments without evaluating or rewriting their order.
fn contains_lambda(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Range { .. } | ast::ExprKind::For { .. } | ast::ExprKind::With { .. } => {
            iteration::source_children(expr)
                .into_iter()
                .any(contains_lambda)
        }
        ast::ExprKind::Lambda { .. } => true,
        ast::ExprKind::Map(entries) => entries
            .iter()
            .any(|(k, v)| contains_lambda(k) || contains_lambda(v)),
        ast::ExprKind::RecordUpdate { value, fields } => {
            contains_lambda(value) || fields.iter().any(|f| contains_lambda(&f.value))
        }
        ast::ExprKind::Apply { callee, args } => {
            contains_lambda(callee) || args.iter().any(|arg| contains_lambda(arg))
        }
        ast::ExprKind::Call { args, .. } | ast::ExprKind::GlobalCall { args, .. } => {
            args.iter().any(|arg| contains_lambda(arg))
        }
        ast::ExprKind::Tuple(args) | ast::ExprKind::List(args) => args.iter().any(contains_lambda),
        ast::ExprKind::Pipe { value, args, .. } | ast::ExprKind::GlobalPipe { value, args, .. } => {
            contains_lambda(value) || args.iter().any(|arg| contains_lambda(arg))
        }
        ast::ExprKind::Return(value)
        | ast::ExprKind::Defer(value)
        | ast::ExprKind::Try(value)
        | ast::ExprKind::Unary { value, .. }
        | ast::ExprKind::Field { value, .. } => contains_lambda(value),
        ast::ExprKind::PostfixIf {
            value: left,
            condition: right,
        }
        | ast::ExprKind::Binary { left, right, .. } => {
            contains_lambda(left) || contains_lambda(right)
        }
        ast::ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            contains_lambda(condition)
                || contains_lambda(then_branch)
                || else_branch.as_deref().is_some_and(contains_lambda)
        }
        ast::ExprKind::Interpolate(parts) | ast::ExprKind::MultilineString(parts) => parts
            .iter()
            .any(|p| matches!(p, ast::StringPart::Value(value) if contains_lambda(value))),
        ast::ExprKind::Match { value, arms } => {
            contains_lambda(value)
                || arms.iter().any(|a| {
                    a.guard.as_ref().is_some_and(contains_lambda) || contains_lambda(&a.body)
                })
        }
        ast::ExprKind::ConditionMatch(arms) => arms
            .iter()
            .any(|a| a.condition.as_ref().is_some_and(contains_lambda) || contains_lambda(&a.body)),
        ast::ExprKind::Block(stmts) => stmts.iter().any(|s| match s {
            ast::Stmt::LetElse {
                value, else_branch, ..
            } => contains_lambda(value) || contains_lambda(else_branch),
            ast::Stmt::Let { value, .. }
            | ast::Stmt::LetPattern { value, .. }
            | ast::Stmt::Expr(value) => contains_lambda(value),
        }),
        _ => false,
    }
}

/// Preserve the operation responsible for a contextual type constraint in diagnostics.
pub(super) fn context(mut error: Diagnostic, label: &str) -> Diagnostic {
    error.message = format!("{label}: {}", error.message);
    error
}
