//! Checked schemes supplement original source headers without executing user code.
use super::*;
use crate::{
    Type,
    check::editor::{self, FunctionInfo},
    presentation,
};
use std::collections::{BTreeSet, HashMap};

/// Typecheck a standalone source library once and render its final reusable signatures.
/// Imports must be module-resolved by callers using render_with_schemes instead.
pub fn render_inferred(source: &str, title: &str, output: Output) -> Result<String, Diagnostic> {
    let program = parse::parse(source)?;
    let schemes = editor::function_schemes(&program)?;
    render_with_schemes(source, title, output, Some(&schemes))
}

/// Render source with optional externally checked schemes at original source-local spans.
/// Callers must supply unmodified schemes from the same source snapshot.
/// Shape/identity/size checks do not attest caller-supplied semantic types.
pub fn render_with_schemes(
    source: &str,
    title: &str,
    output: Output,
    schemes: Option<&[FunctionInfo]>,
) -> Result<String, Diagnostic> {
    if title.len() > 4096 {
        return Err(limit("documentation title exceeds 4096 bytes"));
    }
    let program = parse::parse(source)?;
    let mut items = declarations(source, &program)?;
    if let Some(schemes) = schemes {
        attach(&program, &mut items, schemes)?;
    }
    render_declarations(title, output, &items)
}

/// Join exact declaration anchors, rejecting duplicate or unmatched metadata rather than guessing names.
pub(super) fn attach(
    program: &ast::Program,
    items: &mut [Declaration<'_>],
    schemes: &[FunctionInfo],
) -> Result<(), Diagnostic> {
    if schemes.len() > MAX_DECLARATIONS {
        return Err(limit("documentation scheme count exceeds limit"));
    }
    editor::validate_schemes(schemes)?;
    let mut bytes = 0;
    let mut by_anchor = HashMap::new();
    for info in schemes {
        if by_anchor.insert(info.origin.start, info).is_some() {
            return Err(limit("duplicate checked signature anchor"));
        }
    }
    let originals: HashMap<_, _> = program
        .functions
        .iter()
        .map(|f| (f.span.start, f))
        .collect();
    let mut clauses = HashMap::new();
    for function in &program.functions {
        *clauses.entry(function.name.as_str()).or_insert(0) += 1;
    }
    for item in items {
        if let Some(function) = originals.get(&item.span.start) {
            let info = by_anchor
                .remove(&item.span.start)
                .ok_or_else(|| limit("missing checked signature anchor"))?;
            identity(function, info, clauses[function.name.as_str()])?;
            let text = signature(function, info)?;
            bytes += text.len();
            if bytes > MAX_OUTPUT {
                return Err(limit("checked signatures exceed 8 MiB"));
            }
            item.checked = Some(text);
        }
    }
    if !by_anchor.is_empty() {
        return Err(limit("unmatched checked signature anchor"));
    }
    Ok(())
}

/// Reject mismatched source identities; semantic authenticity remains the caller's obligation.
fn identity(
    function: &ast::Function,
    info: &FunctionInfo,
    clauses: usize,
) -> Result<(), Diagnostic> {
    let name_matches = info.name == function.name
        || info
            .name
            .strip_suffix(&function.name)
            .is_some_and(|prefix| prefix.ends_with('.'));
    if info.origin != function.span
        || !name_matches
        || info.parameters.len() != function.params.len()
        || info.clauses != clauses
    {
        return Err(limit("checked signature source identity mismatch"));
    }
    Ok(())
}

/// Preserve one generic-name assignment across a signature and all intrinsic requirements.
fn signature(function: &ast::Function, info: &FunctionInfo) -> Result<String, Diagnostic> {
    let generated: Vec<_> = info
        .generics
        .iter()
        .filter(|name| name.starts_with('$'))
        .cloned()
        .collect();
    let limits = presentation::Limits {
        bytes: 32_768,
        ..Default::default()
    };
    let mut text = presentation::resolved_signature(
        function,
        &info.parameters,
        &info.result,
        &generated,
        limits,
    )?;
    let context = Type::Function(info.parameters.clone(), Box::new(info.result.clone()));
    let mut seen = BTreeSet::new();
    for requirement in &info.requirements {
        let ty =
            presentation::render_type_in_context(&requirement.ty, &context, &generated, limits)?;
        if requirement.message.len() > 32_768 {
            return Err(limit("checked requirement exceeds 32 KiB"));
        }
        let line = format!("\nrequires {ty}: {}", requirement.message);
        if line.len() > 32_768_usize.saturating_sub(text.len()) {
            return Err(limit("checked signature exceeds 32 KiB"));
        }
        if seen.insert(line.clone()) {
            text.push_str(&line);
        }
    }
    Ok(text)
}
