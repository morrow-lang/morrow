//! Normalize typed source clauses before signature inference and specialization.
use super::*;
use std::borrow::Cow;

pub(super) struct Normalized<'a> {
    pub program: Cow<'a, ast::Program>,
    pub dispatch: HashSet<String>,
}

/// Collapse adjacent clauses while retaining every original arm span and lexical binding.
pub(super) fn normalize(program: &ast::Program) -> Checked<Normalized<'_>> {
    let mut functions = Vec::new();
    let mut dispatch = HashSet::new();
    let mut seen = HashSet::new();
    let mut index = 0;
    while index < program.functions.len() {
        let first = &program.functions[index];
        if !seen.insert(first.name.clone()) {
            return Err(Diagnostic::new(
                first.span,
                "function clauses must be adjacent",
            ));
        }
        let start = index;
        index += 1;
        while index < program.functions.len() && program.functions[index].name == first.name {
            index += 1;
        }
        let group = &program.functions[start..index];
        let result = validate_group(group)?;
        let labels = labels::group(group)?;
        if group.len() == 1
            && first.guard.is_none()
            && first
                .params
                .iter()
                .all(|p| matches!(p.pattern.kind, ast::PatternKind::Bind(_)))
        {
            functions.push(first.clone());
        } else {
            functions.push(lower(group, result, labels));
            dispatch.insert(first.name.clone());
        }
    }
    if dispatch.is_empty() {
        return Ok(Normalized {
            program: Cow::Borrowed(program),
            dispatch,
        });
    }
    let mut normalized = program.clone();
    normalized.functions = functions;
    Ok(Normalized {
        program: Cow::Owned(normalized),
        dispatch,
    })
}

/// Require one consistent API; a supplied return annotation belongs to the complete group.
fn validate_group(group: &[ast::Function]) -> Checked<Option<Type>> {
    let first = &group[0];
    let mut result = None;
    for function in group {
        for param in &function.params {
            if param.annotation.is_none() {
                return Err(Diagnostic::new(
                    param.span,
                    "parameter patterns require a type annotation; private parameter inference is not implemented yet",
                ));
            }
        }
        if function.group_start != first.group_start {
            return Err(Diagnostic::new(
                function.span,
                "function clauses must be adjacent",
            ));
        }
        if function.public != first.public {
            return Err(Diagnostic::new(
                function.span,
                "function clauses must have the same visibility",
            ));
        }
        if function.params.len() != first.params.len() {
            return Err(Diagnostic::new(
                function.span,
                "function clauses must have the same arity",
            ));
        }
        for (actual, expected) in function.params.iter().zip(&first.params) {
            if actual.annotation != expected.annotation {
                return Err(Diagnostic::new(
                    actual.span,
                    "function clauses must use identical parameter types and generic names",
                ));
            }
        }
        if let Some(annotation) = &function.return_type {
            if result
                .as_ref()
                .is_some_and(|previous| previous != annotation)
            {
                return Err(Diagnostic::new(
                    function.span,
                    "function clauses must agree on their return type annotations",
                ));
            }
            result = Some(annotation.clone());
        }
    }
    Ok(result)
}

/// Build a single match dispatch without adding a function or cleanup boundary.
fn lower(
    group: &[ast::Function],
    result: Option<Type>,
    labels: Vec<Option<ast::ArgumentLabel>>,
) -> ast::Function {
    let mut function = group[0].clone();
    function.params = function
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| ast::Param {
            label: labels[index].clone(),
            pattern: ast::Pattern {
                kind: ast::PatternKind::Bind(format!("$clause_arg{index}")),
                span: param.span,
            },
            annotation: param.annotation.clone(),
            span: param.span,
        })
        .collect();
    let arguments: Vec<_> = function
        .params
        .iter()
        .map(|p| ast::Expr {
            kind: ast::ExprKind::Name(parameter_name(p).into()),
            span: p.span,
        })
        .collect();
    let value = expression_tree(arguments, function.span);
    let arms = group
        .iter()
        .map(|clause| ast::MatchArm {
            pattern: pattern_tree(
                clause.params.iter().map(|p| p.pattern.clone()).collect(),
                clause.span,
            ),
            guard: clause.guard.clone(),
            body: clause.body.clone(),
            span: clause.span,
        })
        .collect();
    function.body = ast::Expr {
        kind: ast::ExprKind::Match {
            value: Box::new(value),
            arms,
        },
        span: function.span,
    };
    function.return_type = result;
    function.guard = None;
    function
}

/// Keep generated tuple prefixes below the source limit while supporting 255 parameters.
fn expression_tree(values: Vec<ast::Expr>, span: Span) -> ast::Expr {
    if values.len() == 1 {
        return values.into_iter().next().expect("one argument");
    }
    if values.is_empty() {
        return ast::Expr {
            kind: ast::ExprKind::Unit,
            span,
        };
    }
    let fields = if values.len() <= 64 {
        values
    } else {
        values
            .chunks(64)
            .map(|chunk| ast::Expr {
                kind: ast::ExprKind::Tuple(chunk.to_vec()),
                span,
            })
            .collect()
    };
    ast::Expr {
        kind: ast::ExprKind::Tuple(fields),
        span,
    }
}

/// Mirror dispatch value grouping exactly, including singleton final chunks.
fn pattern_tree(values: Vec<ast::Pattern>, span: Span) -> ast::Pattern {
    if values.len() == 1 {
        return values.into_iter().next().expect("one pattern");
    }
    let fields = if values.len() <= 64 {
        values
    } else {
        values
            .chunks(64)
            .map(|chunk| ast::Pattern {
                kind: ast::PatternKind::Tuple(chunk.to_vec()),
                span,
            })
            .collect()
    };
    ast::Pattern {
        kind: ast::PatternKind::Tuple(fields),
        span,
    }
}

/// Read only normalized parameters; source validation always precedes these accessors.
pub(super) fn parameter_type(param: &ast::Param) -> &Type {
    param
        .annotation
        .as_ref()
        .expect("normalized parameter annotation")
}

/// Source pattern parameters become inaccessible generated names before ordinary checking.
pub(super) fn parameter_name(param: &ast::Param) -> &str {
    let ast::PatternKind::Bind(name) = &param.pattern.kind else {
        unreachable!("normalized parameter binding")
    };
    name
}

/// Hidden argument reads do not discharge the source clause's Result obligations.
pub(super) fn validate_dispatch(body: &ir::Expr, registry: &nominal::Registry) -> Checked<()> {
    let ir::ExprKind::Match { value, arms } = &body.kind else {
        return Err(Diagnostic::new(
            body.span,
            "invalid normalized function dispatch",
        ));
    };
    for arm in arms {
        control::pattern_discards(&arm.pattern, &value.ty, arm.span, registry)?;
    }
    reject_unused_results(body, &[], registry)
}
