//! Source dependency groups for bounded, caller-independent signature inference.
use super::*;
use std::ops::Range;

#[derive(Debug)]
pub(super) struct Group {
    pub name: String,
    pub group_start: usize,
    pub span: Span,
    pub clauses: Range<usize>,
    pub callees: Vec<usize>,
}
#[derive(Debug)]
pub(super) struct Graph {
    pub groups: Vec<Group>,
    pub components: Vec<Vec<usize>>,
    pub work: usize,
}
struct Budget(usize);
impl Budget {
    /// Charge traversal, lookup, copied names and graph algorithms to one aggregate limit.
    fn charge(&mut self, work: usize, span: Span) -> Checked<()> {
        self.0 = self
            .0
            .checked_sub(work)
            .ok_or_else(|| Diagnostic::new(span, "dependency work limit exceeded"))?;
        Ok(())
    }
}

/// Analyze validated source groups; edges refer to source-order group indices.
#[cfg(test)]
pub(super) fn analyze(program: &ast::Program) -> Checked<Graph> {
    analyze_with_work(program, 0)
}

/// Alias expansion and dependency analysis spend one aggregate inference budget.
pub(super) fn analyze_with_work(program: &ast::Program, work: usize) -> Checked<Graph> {
    analyze_mode(program, work, false)
}
/// Direct lexical calls owned by each function exclude separately executed closure bodies.
pub(super) fn owned_calls(program: &ast::Program) -> Checked<Graph> {
    analyze_mode(program, 0, true)
}
fn analyze_mode(program: &ast::Program, work: usize, owned_calls: bool) -> Checked<Graph> {
    preflight::check(program)?;
    let mut budget = Budget(MAX_EXPR_COUNT * 4);
    budget.charge(work, Span::default())?;
    let (mut groups, names) = groups(program, &mut budget)?;
    for group in &mut groups {
        let mut walker = Walker {
            owned_calls,
            tuples: HashSet::new(),
            names: &names,
            locals: HashMap::new(),
            scopes: Vec::new(),
            pending: Vec::new(),
            edges: HashSet::new(),
            budget: &mut budget,
            span: group.span,
        };
        for clause in &program.functions[group.clauses.clone()] {
            walker.clause(clause)?;
        }
        group.callees = walker.edges.into_iter().collect();
        sort_indices(&mut group.callees, &mut budget, group.span)?;
    }
    let components = components(&groups, &mut budget)?;
    Ok(Graph {
        groups,
        components,
        work: MAX_EXPR_COUNT * 4 - budget.0,
    })
}

/// Reject separated or conflicting identities rather than guessing which declaration is meant.
fn groups<'a>(
    program: &'a ast::Program,
    budget: &mut Budget,
) -> Checked<(Vec<Group>, HashMap<&'a str, usize>)> {
    let mut groups = Vec::new();
    let mut names = HashMap::new();
    let mut index = 0;
    while index < program.functions.len() {
        let first = &program.functions[index];
        budget.charge(first.name.len() + 1, first.span)?;
        if names.insert(first.name.as_str(), groups.len()).is_some() {
            return Err(Diagnostic::new(
                first.span,
                "dependency function clauses must be adjacent",
            ));
        }
        let start = index;
        index += 1;
        while index < program.functions.len() && program.functions[index].name == first.name {
            budget.charge(1, program.functions[index].span)?;
            if program.functions[index].group_start != first.group_start {
                return Err(Diagnostic::new(
                    program.functions[index].span,
                    "dependency clause identity mismatch",
                ));
            }
            index += 1;
        }
        groups.push(Group {
            name: first.name.clone(),
            group_start: first.group_start,
            span: first.span,
            clauses: start..index,
            callees: Vec::new(),
        });
    }
    Ok((groups, names))
}

/// Account for deterministic sorting without depending on hash iteration order.
fn sort_indices(values: &mut [usize], budget: &mut Budget, span: Span) -> Checked<()> {
    let levels = usize::BITS as usize - values.len().leading_zeros() as usize;
    budget.charge(values.len().saturating_mul(levels), span)?;
    values.sort_unstable();
    Ok(())
}

enum Task<'a> {
    Expression(&'a ast::Expr),
    Statement(&'a ast::Stmt),
    Pattern(&'a ast::Pattern),
    Name(&'a str, Span),
    Arm(&'a ast::MatchArm),
    Enter,
    Leave,
}
struct Walker<'a, 'b> {
    owned_calls: bool,
    tuples: HashSet<usize>,
    names: &'b HashMap<&'a str, usize>,
    locals: HashMap<&'a str, usize>,
    scopes: Vec<Vec<&'a str>>,
    pending: Vec<Task<'a>>,
    edges: HashSet<usize>,
    budget: &'b mut Budget,
    span: Span,
}

/// Reuse lexical dependency resolution to activate an implicit prelude only for free names.
pub(super) fn uses_trait_prelude(program: &ast::Program) -> Checked<bool> {
    let names: HashMap<_, _> = [
        "show",
        "eq",
        "neq",
        "compare",
        "clone",
        "to_json",
        "from_json",
        "Less",
        "Equal",
        "Greater",
    ]
    .into_iter()
    .filter(|name| {
        !program.functions.iter().any(|f| f.name == *name)
            && !program
                .types
                .iter()
                .flat_map(|d| &d.variants)
                .any(|v| v.name == *name)
    })
    .enumerate()
    .map(|(index, name)| (name, index))
    .collect();
    let mut budget = Budget(MAX_EXPR_COUNT * 4);
    let mut walker = Walker {
        owned_calls: false,
        tuples: HashSet::new(),
        names: &names,
        locals: HashMap::new(),
        scopes: Vec::new(),
        pending: Vec::new(),
        edges: HashSet::new(),
        budget: &mut budget,
        span: Span::default(),
    };
    for function in &program.functions {
        walker.span = function.span;
        walker.clause(function)?;
        if !walker.edges.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Generate tuple instances only for arities present in source, retaining the language's bound.
pub(super) fn trait_tuple_arities(
    program: &ast::Program,
) -> Checked<std::collections::BTreeSet<usize>> {
    let names = HashMap::new();
    let mut budget = Budget(MAX_EXPR_COUNT * 4);
    let mut walker = Walker {
        owned_calls: false,
        tuples: HashSet::new(),
        names: &names,
        locals: HashMap::new(),
        scopes: Vec::new(),
        pending: Vec::new(),
        edges: HashSet::new(),
        budget: &mut budget,
        span: Span::default(),
    };
    for function in &program.functions {
        walker.span = function.span;
        walker.clause(function)?;
    }
    let mut tuples = walker.tuples;
    let mut pending: Vec<_> = program
        .functions
        .iter()
        .flat_map(|f| {
            f.params
                .iter()
                .filter_map(|p| p.annotation.as_ref())
                .chain(f.return_type.as_ref())
        })
        .chain(
            program
                .types
                .iter()
                .flat_map(|d| &d.variants)
                .flat_map(|v| &v.fields)
                .map(|f| &f.ty),
        )
        .chain(program.newtypes.iter().map(|d| &d.inner))
        .chain(program.aliases.iter().map(|a| &a.target))
        .collect();
    while let Some(ty) = pending.pop() {
        budget.charge(1, Span::default())?;
        match ty {
            Type::Tuple(items) => {
                tuples.insert(items.len());
                pending.extend(items);
            }
            Type::List(a) | Type::Option(a) | Type::Pid(a) => pending.push(a),
            Type::Map(a, b) | Type::Result(a, b) | Type::ActorFunction(a, b) => {
                pending.extend([a.as_ref(), b.as_ref()])
            }
            Type::Named(_, args) | Type::Union(args) => pending.extend(args),
            Type::Function(args, result) => {
                pending.extend(args);
                pending.push(result);
            }
            _ => {}
        }
    }
    Ok(tuples.into_iter().filter(|n| *n > 0).collect())
}
impl<'a> Walker<'a, '_> {
    /// Clause binders are independent even though dependencies share one function identity.
    fn clause(&mut self, function: &'a ast::Function) -> Checked<()> {
        self.pending.push(Task::Leave);
        self.pending.push(Task::Expression(&function.body));
        if let Some(guard) = &function.guard {
            self.pending.push(Task::Expression(guard));
        }
        for param in function.params.iter().rev() {
            self.pending.push(Task::Pattern(&param.pattern));
        }
        self.pending.push(Task::Enter);
        while let Some(task) = self.pending.pop() {
            self.budget.charge(1, self.span)?;
            match task {
                Task::Expression(expr) => self.expression(expr)?,
                Task::Statement(stmt) => self.statement(stmt),
                Task::Pattern(pattern) => self.pattern(pattern),
                Task::Name(name, span) => self.bind(name, span)?,
                Task::Arm(arm) => self.arm(arm),
                Task::Enter => self.scopes.push(Vec::new()),
                Task::Leave => self.leave(),
            }
        }
        Ok(())
    }

    /// A local dotted root takes precedence over a qualified source function name.
    fn reference(&mut self, name: &str, span: Span) -> Checked<()> {
        self.budget.charge(name.len() + 1, span)?;
        let root = name.split('.').next().unwrap_or(name);
        if !self.locals.contains_key(root) {
            self.reference_path(name);
        }
        Ok(())
    }

    /// A resolved declaration edge is unaffected by canonical-prefix local bindings.
    fn global_reference(&mut self, name: &str, span: Span) -> Checked<()> {
        self.budget.charge(name.len() + 1, span)?;
        self.reference_path(name);
        Ok(())
    }

    /// A global constant may be followed by record fields in one dotted token.
    fn reference_path(&mut self, mut name: &str) {
        loop {
            if let Some(index) = self.names.get(name) {
                self.edges.insert(*index);
                return;
            }
            let Some((prefix, _)) = name.rsplit_once('.') else {
                return;
            };
            name = prefix;
        }
    }

    /// Store borrowed names with shadow counts; wildcard bindings never become accessible.
    fn bind(&mut self, name: &'a str, span: Span) -> Checked<()> {
        self.budget.charge(name.len() + 1, span)?;
        if name != "_" {
            *self.locals.entry(name).or_default() += 1;
            self.scopes
                .last_mut()
                .expect("binding has lexical scope")
                .push(name);
        }
        Ok(())
    }

    /// Restore precisely the bindings introduced by this lexical scope.
    fn leave(&mut self) {
        for name in self.scopes.pop().expect("balanced lexical scope") {
            let count = self.locals.get_mut(name).expect("registered local binding");
            *count -= 1;
            if *count == 0 {
                self.locals.remove(name);
            }
        }
    }

    /// Queue a child whose declarations cannot escape into a sibling expression.
    fn scoped(&mut self, expr: &'a ast::Expr) {
        self.pending.push(Task::Leave);
        self.pending.push(Task::Expression(expr));
        self.pending.push(Task::Enter);
    }

    /// Initializers and failure branches run before the successful binding exists.
    fn statement(&mut self, stmt: &'a ast::Stmt) {
        match stmt {
            ast::Stmt::Let {
                name, value, span, ..
            } => {
                self.pending.push(Task::Name(name, *span));
                self.pending.push(Task::Expression(value));
            }
            ast::Stmt::LetElse {
                pattern,
                value,
                else_branch,
                ..
            } => {
                self.pending.push(Task::Pattern(pattern));
                self.scoped(else_branch);
                self.pending.push(Task::Expression(value));
            }
            ast::Stmt::LetPattern { pattern, value, .. } => {
                self.pending.push(Task::Pattern(pattern));
                self.pending.push(Task::Expression(value));
            }
            ast::Stmt::Expr(value) => self.pending.push(Task::Expression(value)),
        }
    }

    /// Bind every nested payload/rest name without interpreting constructors as value calls.
    fn pattern(&mut self, pattern: &'a ast::Pattern) {
        match &pattern.kind {
            ast::PatternKind::Typed { pattern, .. } => self.pending.push(Task::Pattern(pattern)),
            ast::PatternKind::Bind(name) => self.pending.push(Task::Name(name, pattern.span)),
            ast::PatternKind::Constructor {
                binding: Some(name),
                ..
            } => {
                self.pending.push(Task::Name(name, pattern.span));
            }
            ast::PatternKind::Tuple(fields) | ast::PatternKind::NamedConstructor { fields, .. } => {
                self.pending.extend(fields.iter().rev().map(Task::Pattern));
            }
            ast::PatternKind::List { prefix, rest } => {
                if let Some(rest) = rest {
                    self.pending.push(Task::Pattern(rest));
                }
                self.pending.extend(prefix.iter().rev().map(Task::Pattern));
            }
            ast::PatternKind::TupleRest { prefix, rest } => {
                self.pending.push(Task::Pattern(rest));
                self.pending.extend(prefix.iter().rev().map(Task::Pattern));
            }
            ast::PatternKind::Wildcard
            | ast::PatternKind::Int(_)
            | ast::PatternKind::Bool(_)
            | ast::PatternKind::String(_)
            | ast::PatternKind::Constructor { binding: None, .. } => {}
        }
    }

    /// A pattern binds before its guard and body, and ends before the next arm.
    fn arm(&mut self, arm: &'a ast::MatchArm) {
        self.pending.push(Task::Leave);
        self.pending.push(Task::Expression(&arm.body));
        if let Some(guard) = &arm.guard {
            self.pending.push(Task::Expression(guard));
        }
        self.pending.push(Task::Pattern(&arm.pattern));
        self.pending.push(Task::Enter);
    }

    /// Handle lexical boundaries explicitly; the remaining expressions only carry references.
    fn expression(&mut self, expr: &'a ast::Expr) -> Checked<()> {
        if let ast::ExprKind::Tuple(items) = &expr.kind {
            self.tuples.insert(items.len());
        }
        match &expr.kind {
            ast::ExprKind::Block(stmts) => {
                self.pending.push(Task::Leave);
                self.pending.extend(stmts.iter().rev().map(Task::Statement));
                self.pending.push(Task::Enter);
            }
            ast::ExprKind::Lambda { .. } if self.owned_calls => {}
            ast::ExprKind::Lambda { params, body } => {
                self.pending.push(Task::Leave);
                self.pending.push(Task::Expression(body));
                self.pending
                    .extend(params.iter().rev().map(|p| Task::Name(&p.name, p.span)));
                self.pending.push(Task::Enter);
            }
            ast::ExprKind::For {
                pattern,
                iterable,
                body,
            } => {
                self.pending.push(Task::Leave);
                self.pending.push(Task::Expression(body));
                self.pending.push(Task::Pattern(pattern));
                self.pending.push(Task::Enter);
                self.pending.push(Task::Expression(iterable));
            }
            ast::ExprKind::Receive { arms, timeout } => {
                if let Some((duration, body)) = timeout {
                    self.scoped(body);
                    self.pending.push(Task::Expression(duration));
                }
                self.pending.extend(arms.iter().rev().map(Task::Arm));
            }
            ast::ExprKind::Match { value, arms } => {
                self.pending.extend(arms.iter().rev().map(Task::Arm));
                self.pending.push(Task::Expression(value));
            }
            ast::ExprKind::With {
                bindings,
                body,
                arms,
            } => self.with(bindings, body, arms.as_deref()),
            ast::ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if let Some(branch) = else_branch {
                    self.scoped(branch);
                }
                self.scoped(then_branch);
                self.pending.push(Task::Expression(condition));
            }
            ast::ExprKind::ConditionMatch(arms) => {
                for arm in arms.iter().rev() {
                    self.scoped(&arm.body);
                    if let Some(condition) = &arm.condition {
                        self.pending.push(Task::Expression(condition));
                    }
                }
            }
            ast::ExprKind::Defer(value) => self.scoped(value),
            _ => self.plain(expr)?,
        }
        Ok(())
    }

    /// Successful with-bindings are sequential; each error arm sees the original outer scope.
    fn with(
        &mut self,
        bindings: &'a [ast::WithBinding],
        body: &'a ast::Expr,
        arms: Option<&'a [ast::MatchArm]>,
    ) {
        if let Some(arms) = arms {
            self.pending.extend(arms.iter().rev().map(Task::Arm));
        }
        self.pending.push(Task::Leave);
        self.pending.push(Task::Expression(body));
        for binding in bindings.iter().rev() {
            self.pending.push(Task::Pattern(&binding.pattern));
            self.pending.push(Task::Expression(&binding.value));
        }
        self.pending.push(Task::Enter);
    }

    /// Queue map key/value expressions in source order without changing dependency scopes.
    fn map_entries(&mut self, entries: &'a [(ast::Expr, ast::Expr)]) {
        for (key, value) in entries.iter().rev() {
            self.pending.push(Task::Expression(value));
            self.pending.push(Task::Expression(key));
        }
    }

    /// Queue source-written argument values; labels introduce no dependency edges.
    fn arguments(&mut self, args: &'a [ast::Argument]) {
        self.pending
            .extend(args.iter().rev().map(|arg| Task::Expression(&arg.value)));
    }

    /// Queue explicit global edges independently of canonical-prefix local bindings.
    fn global(&mut self, expr: &'a ast::Expr) -> Checked<bool> {
        use ast::ExprKind::*;
        match &expr.kind {
            GlobalName { resolved, .. } => {
                if !self.owned_calls {
                    self.global_reference(resolved, expr.span)?;
                }
            }
            GlobalCall { resolved, args, .. } => {
                self.global_reference(resolved, expr.span)?;
                self.arguments(args);
            }
            GlobalPipe {
                value,
                resolved,
                args,
                ..
            } => {
                self.global_reference(resolved, expr.span)?;
                self.arguments(args);
                self.pending.push(Task::Expression(value));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Queue value children exhaustively so new syntax cannot silently lose dependencies.
    fn string_parts(&mut self, parts: &'a [ast::StringPart]) {
        for part in parts.iter().rev() {
            if let ast::StringPart::Value(value) = part {
                self.pending.push(Task::Expression(value));
            }
        }
    }

    /// Queue ordinary expression children in source evaluation order.
    fn plain(&mut self, expr: &'a ast::Expr) -> Checked<()> {
        use ast::ExprKind::*;
        if self.global(expr)? {
            return Ok(());
        }
        match &expr.kind {
            TypeTarget(_) => {}
            Name(name) => {
                if !self.owned_calls {
                    self.reference(name, expr.span)?;
                }
            }
            Call { name, args } => {
                self.reference(name, expr.span)?;
                self.arguments(args);
            }
            Pipe {
                value, name, args, ..
            } => {
                self.reference(name, expr.span)?;
                self.arguments(args);
                self.pending.push(Task::Expression(value));
            }
            Apply { callee, args } => {
                self.arguments(args);
                if self.owned_calls {
                    match &callee.kind {
                        Name(name) => self.reference(name, callee.span)?,
                        GlobalName { resolved, .. } => {
                            self.global_reference(resolved, callee.span)?
                        }
                        _ => self.pending.push(Task::Expression(callee)),
                    }
                } else {
                    self.pending.push(Task::Expression(callee));
                }
            }
            ForeignCall { args: values, .. } | Tuple(values) | List(values) => self
                .pending
                .extend(values.iter().rev().map(Task::Expression)),
            Map(entries) => self.map_entries(entries),
            RecordUpdate { value, fields } => {
                self.pending
                    .extend(fields.iter().rev().map(|f| Task::Expression(&f.value)));
                self.pending.push(Task::Expression(value));
            }
            Interpolate(parts) | MultilineString(parts) => self.string_parts(parts),
            Field { value, .. } | Unary { value, .. } | Try(value) | Return(value) => {
                self.pending.push(Task::Expression(value));
            }
            Binary { left, right, .. }
            | PostfixIf {
                condition: left,
                value: right,
            }
            | Range {
                start: left,
                end: right,
                ..
            } => {
                self.pending.push(Task::Expression(right));
                self.pending.push(Task::Expression(left));
            }
            GlobalName { .. } | GlobalCall { .. } | GlobalPipe { .. } => {
                unreachable!("global handled above")
            }
            Int(_) | Float(_) | Bool(_) | String(_) | Unit | Break | Continue => {}
            Block(_)
            | Lambda { .. }
            | For { .. }
            | Receive { .. }
            | Match { .. }
            | With { .. }
            | If { .. }
            | ConditionMatch(_)
            | Defer(_) => unreachable!("scoped expression handled first"),
        }
        Ok(())
    }
}

/// Kosaraju traversal uses explicit DFS stacks, including for 4096-node recursive components.
fn components(groups: &[Group], budget: &mut Budget) -> Checked<Vec<Vec<usize>>> {
    let finish = finish_order(groups, budget)?;
    let mut incoming = vec![Vec::new(); groups.len()];
    for (caller, group) in groups.iter().enumerate() {
        for &callee in &group.callees {
            budget.charge(1, group.span)?;
            incoming[callee].push(caller);
        }
    }
    let mut seen = vec![false; groups.len()];
    let mut components = Vec::new();
    for start in finish.into_iter().rev() {
        if seen[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![start];
        seen[start] = true;
        while let Some(node) = pending.pop() {
            budget.charge(1, groups[node].span)?;
            component.push(node);
            for &caller in &incoming[node] {
                budget.charge(1, groups[node].span)?;
                if !seen[caller] {
                    seen[caller] = true;
                    pending.push(caller);
                }
            }
        }
        sort_indices(&mut component, budget, groups[start].span)?;
        components.push(component);
    }
    components.reverse();
    Ok(components)
}

/// Obtain forward DFS finish order without recursion on the host stack.
fn finish_order(groups: &[Group], budget: &mut Budget) -> Checked<Vec<usize>> {
    let mut seen = vec![false; groups.len()];
    let mut finish = Vec::new();
    for start in 0..groups.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut pending = vec![(start, 0)];
        while let Some((node, cursor)) = pending.last_mut() {
            budget.charge(1, groups[*node].span)?;
            if let Some(&callee) = groups[*node].callees.get(*cursor) {
                *cursor += 1;
                if !seen[callee] {
                    seen[callee] = true;
                    pending.push((callee, 0));
                }
            } else {
                finish.push(*node);
                pending.pop();
            }
        }
    }
    Ok(finish)
}

#[cfg(test)]
mod tests;
