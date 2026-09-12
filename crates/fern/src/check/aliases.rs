//! Transparent aliases expand before nominal identity checking without introducing values.
use super::*;
use std::borrow::Cow;
mod rewrite;

pub(super) struct Expanded<'a> {
    pub program: Cow<'a, ast::Program>,
    pub work: usize,
}
struct Expander<'a> {
    aliases: HashMap<&'a str, &'a ast::TypeAlias>,
    active: HashSet<&'a str>,
    work: usize,
}

/// Preserve the no-alias path; normalize a bounded copy while retaining declaration identities.
pub(super) fn expand(program: &ast::Program) -> Checked<Expanded<'_>> {
    if program.aliases.is_empty() {
        return Ok(Expanded {
            program: Cow::Borrowed(program),
            work: 0,
        });
    }
    let aliases = declarations(program)?;
    let mut expander = Expander {
        aliases,
        active: HashSet::new(),
        work: 0,
    };
    for alias in &program.aliases {
        expander.charge(alias.name.len() + 1, alias.span)?;
        for parameter in &alias.parameters {
            expander.charge(parameter.len() + 1, alias.span)?;
        }
    }
    let mut output = program.clone();
    for alias in &mut output.aliases {
        alias.target = expander.expand(&alias.target, alias.span)?;
    }
    for declaration in &mut output.newtypes {
        declaration.inner = expander.expand(&declaration.inner, declaration.inner_span)?;
    }
    for declaration in &mut output.types {
        for field in declaration.variants.iter_mut().flat_map(|v| &mut v.fields) {
            field.ty = expander.expand(&field.ty, field.span)?;
        }
    }
    for function in &mut output.functions {
        for parameter in &mut function.params {
            expander.annotation(&mut parameter.annotation, parameter.span)?;
        }
        expander.annotation(&mut function.return_type, function.span)?;
        rewrite::expression(&mut function.body, &mut expander)?;
        if let Some(guard) = &mut function.guard {
            rewrite::expression(guard, &mut expander)?;
        }
    }
    Ok(Expanded {
        program: Cow::Owned(output),
        work: expander.work,
    })
}

/// Validate even unused alias targets after the expanded nominal declarations are known.
pub(super) fn validate(program: &ast::Program, registry: &nominal::Registry) -> Checked<()> {
    for alias in &program.aliases {
        registry.validate(
            &alias.target,
            &alias.parameters.iter().cloned().collect(),
            alias.span,
        )?;
    }
    Ok(())
}

/// Reject namespace collisions and undeclared formals before references become transparent.
fn declarations(program: &ast::Program) -> Checked<HashMap<&str, &ast::TypeAlias>> {
    let mut occupied: HashSet<&str> = HashSet::new();
    for ty in &program.types {
        occupied.insert(&ty.name);
    }
    for decl in &program.newtypes {
        occupied.insert(&decl.name);
    }
    let mut aliases = HashMap::new();
    for alias in &program.aliases {
        let leaf = alias.name.rsplit('.').next().unwrap_or(&alias.name);
        if reserved(&alias.name)
            || matches!(leaf, "Int" | "Bool" | "Float" | "Unit" | "Never")
            || !leaf.chars().next().is_some_and(char::is_uppercase)
        {
            return Err(Diagnostic::new(
                alias.span,
                "type alias name must be capitalized and not reserved",
            ));
        }
        if !occupied.insert(&alias.name) {
            return Err(Diagnostic::new(
                alias.span,
                format!("duplicate type alias declaration '{}'", alias.name),
            ));
        }
        let names: HashSet<_> = alias.parameters.iter().cloned().collect();
        if names.len() != alias.parameters.len()
            || alias.parameters.iter().any(|n| !generic_name(n))
        {
            return Err(Diagnostic::new(
                alias.span,
                "duplicate or invalid alias type parameter",
            ));
        }
        if nominal::generics([alias.target.clone()])
            .iter()
            .any(|name| !names.contains(name))
        {
            return Err(Diagnostic::new(
                alias.span,
                "undeclared type variable in alias target",
            ));
        }
        aliases.insert(alias.name.as_str(), alias);
    }
    Ok(aliases)
}

/// Alias formals follow the source lowercase generic-variable spelling convention.
fn generic_name(name: &str) -> bool {
    name.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

impl Expander<'_> {
    /// Charge copied identifiers and each expanded type node to the shared inference budget.
    fn charge(&mut self, amount: usize, span: Span) -> Checked<()> {
        self.work = self.work.saturating_add(amount);
        if self.work > MAX_EXPR_COUNT * 4 {
            return Err(Diagnostic::new(
                span,
                "alias expansion inference work limit exceeded",
            ));
        }
        Ok(())
    }
    /// Each retained annotation receives its own structural bound as well as aggregate accounting.
    fn expand(&mut self, ty: &Type, span: Span) -> Checked<Type> {
        let mut nodes = MAX_TYPE_NODES;
        self.ty(ty, &HashMap::new(), span, 0, &mut nodes)
    }
    /// Rewrite an optional local/signature annotation without altering its source span.
    fn annotation(&mut self, value: &mut Option<Type>, span: Span) -> Checked<()> {
        *value = value.as_ref().map(|ty| self.expand(ty, span)).transpose()?;
        Ok(())
    }
    /// Simultaneous substitution never captures a caller generic with another alias formal.
    fn ty(
        &mut self,
        ty: &Type,
        values: &HashMap<String, Type>,
        span: Span,
        depth: usize,
        nodes: &mut usize,
    ) -> Checked<Type> {
        if depth >= MAX_TYPE_DEPTH || *nodes == 0 {
            return Err(Diagnostic::new(
                span,
                "type alias expansion nesting or size limit exceeded",
            ));
        }
        *nodes -= 1;
        self.charge(1, span)?;
        Ok(match ty {
            Type::Generic(name) => {
                self.charge(name.len(), span)?;
                if let Some(value) = values.get(name) {
                    self.ty(value, &HashMap::new(), span, depth + 1, nodes)?
                } else {
                    ty.clone()
                }
            }
            Type::Named(name, arguments) => {
                self.named(name, arguments, values, span, depth, nodes)?
            }
            Type::Pid(a) => Type::Pid(Box::new(self.ty(a, values, span, depth + 1, nodes)?)),
            Type::List(a) => Type::List(Box::new(self.ty(a, values, span, depth + 1, nodes)?)),
            Type::Option(a) => {
                Type::Option(Box::new(self.ty(a, values, span, depth + 1, nodes)?))
            }
            Type::Result(a, b) => Type::Result(
                Box::new(self.ty(a, values, span, depth + 1, nodes)?),
                Box::new(self.ty(b, values, span, depth + 1, nodes)?),
            ),
            Type::Map(a, b) => Type::Map(
                Box::new(self.ty(a, values, span, depth + 1, nodes)?),
                Box::new(self.ty(b, values, span, depth + 1, nodes)?),
            ),
            Type::Union(args) => {
                self.charge(crate::unions::cost(ty, span)?, span)?;
                crate::unions::make(self.arguments(args, values, span, depth, nodes)?, span)?
            }
            Type::Tuple(args) => Type::Tuple(self.arguments(args, values, span, depth, nodes)?),
            Type::Function(args, result) => Type::Function(
                self.arguments(args, values, span, depth, nodes)?,
                Box::new(self.ty(result, values, span, depth + 1, nodes)?),
            ),
            _ => ty.clone(),
        })
    }
    /// Expand arguments before binding formals, preserving simultaneous replacement semantics.
    fn arguments(
        &mut self,
        args: &[Type],
        values: &HashMap<String, Type>,
        span: Span,
        depth: usize,
        nodes: &mut usize,
    ) -> Checked<Vec<Type>> {
        args.iter()
            .map(|ty| self.ty(ty, values, span, depth + 1, nodes))
            .collect()
    }
    /// Follow only transparent references; nominal fields do not participate in alias cycles.
    fn named(
        &mut self,
        name: &str,
        args: &[Type],
        values: &HashMap<String, Type>,
        span: Span,
        depth: usize,
        nodes: &mut usize,
    ) -> Checked<Type> {
        self.charge(name.len(), span)?;
        let args = self.arguments(args, values, span, depth, nodes)?;
        let Some(alias) = self.aliases.get(name).copied() else {
            return Ok(Type::Named(name.into(), args));
        };
        if args.len() != alias.parameters.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "type alias '{}' expects {} type argument(s), found {}",
                    name,
                    alias.parameters.len(),
                    args.len()
                ),
            ));
        }
        if !self.active.insert(&alias.name) {
            return Err(Diagnostic::new(
                span,
                format!("transparent type alias cycle through '{name}'"),
            ));
        }
        let values = alias.parameters.iter().cloned().zip(args).collect();
        let result = self.ty(&alias.target, &values, span, depth + 1, nodes);
        self.active.remove(alias.name.as_str());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expanded_alias_work_is_carried_into_dependency_analysis() {
        let program = crate::parse::parse("type Id = Int\nfn main(): ()\n").unwrap();
        let expanded = expand(&program).unwrap();
        assert!(expanded.work > 0);
        let plain = dependencies::analyze(&expanded.program).unwrap();
        let combined = dependencies::analyze_with_work(&expanded.program, expanded.work).unwrap();
        assert_eq!(combined.work, plain.work + expanded.work);
        assert!(dependencies::analyze_with_work(&expanded.program, MAX_EXPR_COUNT * 4).is_err());
    }
    #[test]
    fn aggregate_alias_expansion_budget_does_not_reset_between_types() {
        let mut expander = Expander {
            aliases: HashMap::new(),
            active: HashSet::new(),
            work: MAX_EXPR_COUNT * 4 - 1,
        };
        expander.expand(&Type::Int, Span::default()).unwrap();
        let error = expander.expand(&Type::Bool, Span::default()).unwrap_err();
        assert!(error.message.contains("work limit"));
    }
}
