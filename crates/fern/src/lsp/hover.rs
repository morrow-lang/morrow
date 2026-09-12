//! Plain-text semantic hover over a fully checked current-source snapshot.
use super::*;
use crate::presentation::{self, Limits};
use check::editor::{Facts, FunctionInfo};

/// Respond only when both lexical source identity and current checked facts are available.
pub(super) fn hover(
    index: Option<&index::Index<'_>>,
    program: Option<&ast::Program>,
    facts: Option<&Facts>,
) -> Json {
    let (Some(index), Some(program), Some(facts)) = (index, program, facts) else {
        return Json::Null;
    };
    let Some(query) = index.query() else {
        return Json::Null;
    };
    let Some((_, source, local)) = index.location(query.occurrence) else {
        return Json::Null;
    };
    let Some(name) = source.get(local.start..local.end) else {
        return Json::Null;
    };
    let Some((mut text, doc_target)) = (if let Some(label) = &index.label {
        label_description(label, name, facts)
    } else {
        description(program, index.target, index.symbol_name(), name, facts)
    }) else {
        return Json::Null;
    };
    if let Some(target) = doc_target
        && let Some(doc) = owned_documentation(program, target)
        && !doc.text.is_empty()
    {
        text.push_str("\n\n");
        text.push_str(excerpt(&doc.text, 16_384));
        if doc.text.len() > 16_384 {
            text.push_str("\n[excerpt]");
        }
    }
    let text = bounded_text(text);
    object([
        (
            "contents",
            object([("kind", string("plaintext")), ("value", string(text))]),
        ),
        ("range", super::navigation::source_range(source, local)),
    ])
}

/// Labels describe finalized declared parameter schemes, never a call-site substitution.
fn label_description(
    label: &index::LabelSelection,
    name: &str,
    facts: &Facts,
) -> Option<(String, Option<Span>)> {
    let function = facts.function.as_ref()?;
    if function.name != label.function {
        return None;
    }
    let ty = function.parameters.get(label.position)?;
    Some((format!("{name}: {}", type_text(ty, facts).ok()?), None))
}

/// Distinguish reusable declaration signatures from instantiated source occurrence types.
fn description(
    program: &ast::Program,
    binding: Option<Span>,
    symbol: Option<&str>,
    name: &str,
    facts: &Facts,
) -> Option<(String, Option<Span>)> {
    if let Some(info) = &facts.function {
        let function = program.functions.iter().find(|f| f.name == info.name)?;
        let mut text = presentation::resolved_signature(
            function,
            &info.parameters,
            &info.result,
            &generated(info),
            limits(),
        )
        .ok()?;
        if info.clauses > 1 {
            text.push_str(&format!("\n{} clauses", info.clauses));
        }
        if let Some(value) = &facts.value {
            let signature = Type::Function(info.parameters.clone(), Box::new(info.result.clone()));
            if *value != signature {
                text.push_str("\nAt this use: ");
                text.push_str(&type_text(value, facts).ok()?);
            }
        }
        requirements(&mut text, info)?;
        return Some((text, Some(function.span)));
    }
    if let Some(description) = newtype_declaration(program, binding, symbol, name) {
        return Some(description);
    }
    if let Some(ty) = &facts.value {
        let text = if *ty == Type::Never {
            "does not return".into()
        } else {
            type_text(ty, facts).ok()?
        };
        return Some((format!("{name}: {text}"), None));
    }
    declaration(program, binding, symbol, name)
}

/// Newtype type names and constructor values have separate signatures but share declaration docs.
fn newtype_declaration(
    program: &ast::Program,
    target: Option<Span>,
    symbol: Option<&str>,
    name: &str,
) -> Option<(String, Option<Span>)> {
    for decl in &program.newtypes {
        let arguments = decl.parameters.iter().cloned().map(Type::Generic).collect();
        let owner = Type::Named(decl.name.clone(), arguments);
        if target == Some(decl.constructor_span) && symbol == Some(decl.constructor.as_str()) {
            let ty = Type::Function(vec![decl.inner.clone()], Box::new(owner));
            return Some((
                format!("{name}: {}", presentation::render_type(&ty, limits()).ok()?),
                Some(decl.span),
            ));
        }
        if symbol == Some(decl.name.as_str()) {
            let owner = presentation::render_type(&owner, limits()).ok()?;
            let inner = presentation::render_type(&decl.inner, limits()).ok()?;
            return Some((
                format!("newtype {owner} = {}({inner})", decl.constructor),
                Some(decl.span),
            ));
        }
    }
    None
}

/// Declaration-only facts use source types whose validity the ordinary checker already proved.
fn declaration(
    program: &ast::Program,
    target: Option<Span>,
    symbol: Option<&str>,
    name: &str,
) -> Option<(String, Option<Span>)> {
    if let Some(alias) = program
        .aliases
        .iter()
        .find(|alias| symbol == Some(alias.name.as_str()))
    {
        let args = alias
            .parameters
            .iter()
            .cloned()
            .map(Type::Generic)
            .collect();
        let owner =
            presentation::render_type(&Type::Named(alias.name.clone(), args), limits()).ok()?;
        let target = presentation::render_type(&alias.target, limits()).ok()?;
        return Some((format!("type {owner} = {target}"), Some(alias.span)));
    }
    for decl in &program.types {
        let args: Vec<_> = decl.parameters.iter().cloned().map(Type::Generic).collect();
        let owner = Type::Named(decl.name.clone(), args);
        if symbol == Some(decl.name.as_str()) {
            let text = presentation::render_type(&owner, limits()).ok()?;
            return Some((format!("type {text}"), Some(decl.span)));
        }
        for variant in &decl.variants {
            for field in &variant.fields {
                if target.is_some_and(|span| {
                    field.span.start <= span.start && span.end <= field.span.end
                }) {
                    let ty = presentation::render_type(&field.ty, limits()).ok()?;
                    return Some((format!("{name}: {ty}"), None));
                }
            }
            if !decl.record && symbol == Some(variant.name.as_str()) {
                let ty = Type::Function(
                    variant.fields.iter().map(|f| f.ty.clone()).collect(),
                    Box::new(owner),
                );
                return Some((
                    format!("{name}: {}", presentation::render_type(&ty, limits()).ok()?),
                    None,
                ));
            }
        }
    }
    None
}

/// Keep requirements readable without inventing public trait/where syntax.
fn requirements(text: &mut String, info: &FunctionInfo) -> Option<()> {
    let mut seen = std::collections::BTreeSet::new();
    for requirement in &info.requirements {
        let ty = presentation::render_type_in_context(
            &requirement.ty,
            &Type::Function(info.parameters.clone(), Box::new(info.result.clone())),
            &generated(info),
            limits(),
        )
        .ok()?;
        let line = format!("\nRequires {ty}: {}", requirement.message);
        if seen.insert(line.clone()) {
            text.push_str(&line);
        }
        if text.len() > 60_000 {
            return None;
        }
    }
    Some(())
}

/// Render checked local/member types without allowing internal quantified identities to leak.
pub(super) fn type_text(
    ty: &Type,
    facts: &Facts,
) -> std::result::Result<String, crate::Diagnostic> {
    if let Some(context) = &facts.context {
        let scope = Type::Function(context.parameters.clone(), Box::new(context.result.clone()));
        presentation::render_type_in_context(ty, &scope, &generated(context), limits())
    } else {
        presentation::render_type(ty, limits())
    }
}

/// Only checker-registered inferred variables may receive public display aliases.
fn generated(info: &FunctionInfo) -> Vec<String> {
    info.generics
        .iter()
        .filter(|name| name.starts_with('$'))
        .cloned()
        .collect()
}

/// Bound individual presentation fragments below the total hover response budget.
fn limits() -> Limits {
    Limits {
        bytes: 32_768,
        ..Limits::default()
    }
}

/// Charge JSON escape expansion before returning a complete bounded hover response.
fn bounded_text(mut text: String) -> String {
    let mut bytes = 0;
    let cut = text.char_indices().find_map(|(offset, ch)| {
        bytes += if ch < '\u{20}' {
            6
        } else if ch == '"' || ch == '\\' {
            2
        } else {
            ch.len_utf8()
        };
        (bytes > 64_000).then_some(offset)
    });
    if let Some(cut) = cut {
        text.truncate(cut);
        text.push_str("\n[excerpt]");
    }
    text
}

/// Truncate at a valid UTF-8 boundary while retaining literal documentation content.
fn excerpt(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A valid checked member points to the original field declaration's identifier.
pub(super) fn field_definition(index: &index::Index<'_>, facts: Option<&Facts>) -> Option<Span> {
    let query = index.query()?;
    let (_, source, local) = index.location(query.occurrence)?;
    let name = source.get(local.start..local.end)?;
    let origin = facts?.members.iter().find(|m| m.name == name)?.origin?;
    index.identifier(origin, 0)
}

/// Attach a doc block to its next source declaration, never to another same-spelled symbol.
fn owned_documentation(program: &ast::Program, owner: Span) -> Option<&ast::DocComment> {
    let previous = program
        .functions
        .iter()
        .map(|f| f.span.start)
        .chain(program.types.iter().map(|d| d.span.start))
        .chain(program.aliases.iter().map(|d| d.span.start))
        .chain(program.newtypes.iter().map(|d| d.span.start))
        .filter(|start| *start < owner.start)
        .max();
    program.docs.iter().find(|doc| {
        doc.span.end <= owner.start && previous.is_none_or(|start| start < doc.span.end)
    })
}
