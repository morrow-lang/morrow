//! External parameter identities are separate from lexical bindings and generated IR.
use super::*;

pub(crate) struct LabelSelection {
    pub function: String,
    pub position: usize,
}

impl Index<'_> {
    /// Retain the first source contributor to each stable clause interface position.
    pub(super) fn interfaces(&mut self, program: &ast::Program) -> Option<()> {
        let mut groups = BTreeMap::new();
        let mut invalid = std::collections::BTreeSet::new();
        for function in &program.functions {
            self.charge(function.params.len().saturating_add(1), 0)?;
            if function.span == Span::default() {
                continue;
            }
            let identity = (function.group_start, function.params.len(), function.public);
            let group = groups.entry(function.name.clone()).or_insert(identity);
            if *group != identity {
                invalid.insert(function.name.clone());
            }
            let slots = self
                .interfaces
                .entry(function.name.clone())
                .or_insert_with(|| vec![None; function.params.len()]);
            for (slot, param) in slots.iter_mut().zip(&function.params) {
                let label = param.label.clone().or_else(|| match &param.pattern.kind {
                    ast::PatternKind::Bind(name) => Some(ast::ArgumentLabel {
                        name: name.clone(),
                        span: param.pattern.span,
                    }),
                    _ => None,
                });
                if let (Some(old), Some(new)) = (&slot, &label)
                    && old.name != new.name
                {
                    invalid.insert(function.name.clone());
                }
                if slot.is_none() {
                    *slot = label;
                }
            }
        }
        if self.completion_site.is_some() {
            for (name, slots) in &self.interfaces {
                let mut names = std::collections::BTreeSet::new();
                if reserved_callee(name)
                    || slots
                        .iter()
                        .flatten()
                        .any(|label| !names.insert(&label.name))
                {
                    invalid.insert(name.clone());
                }
            }
            for ty in &program.types {
                invalid.extend(ty.variants.iter().map(|v| v.name.clone()));
            }
            invalid.extend(program.newtypes.iter().map(|n| n.constructor.clone()));
            for name in invalid {
                self.interfaces.remove(&name);
            }
        }
        Some(())
    }

    /// Local roots erase source interfaces; resolved global nodes bypass this lexical lookup.
    pub(super) fn source_callee(&self, name: &str, locals: &Bindings) -> Option<String> {
        if locals.contains_key(name.split('.').next()?) {
            return None;
        }
        let canonical = self.visible_values.get(name)?;
        self.interfaces
            .contains_key(canonical)
            .then(|| canonical.clone())
    }

    /// Mark label syntax even when invalid, preventing accidental local/global fallback.
    pub(super) fn argument_label(&mut self, label: &ast::ArgumentLabel, callee: Option<&str>) {
        if !self.contains(label.span) {
            return;
        }
        self.blocked = true;
        self.target = None;
        self.label = None;
        let selected = callee.and_then(|name| {
            self.interfaces
                .get(name)?
                .iter()
                .enumerate()
                .find_map(|(position, item)| {
                    let item = item.as_ref()?;
                    (item.name == label.name).then_some((name, position, item.span))
                })
        });
        if let Some((function, position, target)) = selected {
            self.target = Some(target);
            self.token = Some(label.span);
            self.label = Some(LabelSelection {
                function: function.into(),
                position,
            });
        } else {
            self.token = None;
        }
    }

    /// Include a removed labeled placeholder in the same interface as remaining pipe arguments.
    pub(super) fn pipe(
        &mut self,
        expression: &ast::Expr,
        locals: &Bindings,
        depth: usize,
    ) -> Option<()> {
        let (value, args, label, callee) = match &expression.kind {
            ast::ExprKind::Pipe {
                value,
                name,
                args,
                label,
                ..
            } => (value, args, label, self.source_callee(name, locals)),
            ast::ExprKind::GlobalPipe {
                value,
                resolved,
                args,
                label,
                ..
            } => (value, args, label, Some(resolved.clone())),
            _ => return None,
        };
        self.complete_labels(expression, args, callee.as_deref(), locals);
        self.pipe_reference(expression, locals);
        self.expression(value, locals, depth)?;
        if let Some(label) = label {
            self.argument_label(label, callee.as_deref());
        }
        self.arguments(args, callee.as_deref(), locals, depth)
    }
}

impl<'a> Index<'a> {
    /// Recover only lexical token ranges; argument suggestions never use these as typed facts.
    pub(super) fn label_source(path: &'a Path, text: &'a str, start: usize) -> Option<Source<'a>> {
        Some(Source {
            path,
            text,
            start,
            tokens: parse::identifier_index(text).ok()?,
            annotations: Vec::new(),
            selectors: Vec::new(),
        })
    }

    /// Select one canonical source interface and exclude every other supplied argument position.
    pub(super) fn complete_labels(
        &mut self,
        expression: &ast::Expr,
        _args: &[ast::Argument],
        callee: Option<&str>,
        _locals: &Bindings,
    ) {
        let Some(site) = &self.completion_site else {
            return;
        };
        let Some(range) = site.arguments() else {
            return;
        };
        if expression.span.end != range.end || expression.span.start >= range.start {
            return;
        }
        let Some(interface) = callee.and_then(|name| self.interfaces.get(name)) else {
            return;
        };
        self.call_labels = Some(Vec::new());
        if site.supplied().len() > interface.len() {
            return;
        }
        let mut occupied = vec![false; interface.len()];
        let mut positional = 0;
        for (label, span) in site.supplied() {
            if site.colon() && *span == site.selector() {
                continue;
            }
            let slot = if let Some(label) = label {
                interface
                    .iter()
                    .position(|item| item.as_ref().is_some_and(|i| i.name == *label))
            } else {
                let slot = positional;
                positional += 1;
                Some(slot)
            };
            let Some(slot) = slot.and_then(|slot| occupied.get_mut(slot)) else {
                return;
            };
            if *slot {
                return;
            }
            *slot = true;
        }
        let labels: Vec<_> = interface
            .iter()
            .zip(occupied)
            .filter_map(|(label, used)| {
                label
                    .as_ref()
                    .filter(|l| !used && l.name.starts_with(site.prefix()))
                    .map(|l| l.name.clone())
            })
            .collect();
        self.call_labels = (!labels.is_empty() || site.colon()).then_some(labels);
    }
}

/// Builtins and constructors cannot acquire a source interface through an invalid declaration.
fn reserved_callee(name: &str) -> bool {
    crate::check::builtin(name).is_some()
        || crate::codec_syntax::is_codec(name)
        || crate::runtime::resolve(name).is_some()
        || matches!(name, "Some" | "None" | "Ok" | "Err")
}
