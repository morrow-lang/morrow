//! Source-owned test functions use ordinary checking and isolated native entries.
use crate::{Diagnostic, Span, Type, ast, check, doctest, ir, parse};

/// One original test declaration; adjacent clauses share the first source anchor.
#[derive(Debug)]
pub struct Case {
    pub name: String,
    pub span: Span,
}

/// Discover at most 256 source test_ groups from bounded parsed source.
/// Comments, imported functions and benchmark helpers do not declare unit tests.
pub fn discover(source: &str) -> Result<Vec<Case>, Diagnostic> {
    let parsed = parse::parse(source)?;
    let mut tests = Vec::new();
    let mut previous = None;
    for function in &parsed.functions {
        if !function.name.starts_with("test_") || previous == Some(&function.name) {
            continue;
        }
        previous = Some(&function.name);
        if tests.len() == 256 {
            return Err(Diagnostic::new(
                function.span,
                "unit test limit exceeds 256",
            ));
        }
        tests.push(Case {
            name: function.name.clone(),
            span: function.span,
        });
    }
    Ok(tests)
}

/// Check the complete source library and reject reusable generic tests before specialization.
/// The selected name is an already resolved source declaration identity, not an import spelling.
pub fn prepare(source: &ast::Program, name: &str) -> Result<ir::Program, Diagnostic> {
    let mut program = check::check_test(source, name)?;
    select_entry(&mut program, name)?;
    Ok(program)
}

/// Select one fully checked Unit/Result(Unit,E) function without changing resolved calls.
/// Invalid names, arguments, captures or result shapes leave the supplied IR untouched.
fn select_entry(program: &mut ir::Program, name: &str) -> Result<(), Diagnostic> {
    let mut matches = program
        .functions
        .iter()
        .filter(|function| function.name == name);
    let valid = matches.next().is_some_and(|function| {
        function.params.is_empty()
            && function.captures.is_empty()
            && (function.return_type == Type::Unit
                || matches!(&function.return_type,
                Type::Result(ok, _) if **ok == Type::Unit))
    });
    if !valid || matches.next().is_some() {
        return Err(Diagnostic::new(
            Span::default(),
            format!("test {name} requires zero arguments and Unit or Result(Unit, E) result"),
        ));
    }
    doctest::rename_entry(program, name);
    Ok(())
}
