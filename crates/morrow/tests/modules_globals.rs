use morrow_compiler::modules;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn aliased_globals_keep_identity_when_canonical_root_is_local() {
    let project = Project::new();
    project.write("model.mr", "pub fn value(x: a) -> a: x\n");
    let main = project.write("main.mr", "import model as m\nfn main():\n    let model = 41\n    let direct = m.value(model)\n    let callable = m.value\n    let piped = model |> m.value()\n    let capture = () -> m.value(model)\n    println(direct + callable(piped) + capture())\n");
    let loaded = modules::load(&main).unwrap();
    let checked = morrow_compiler::check::check(&loaded.program).unwrap();
    morrow_compiler::lowering::emit(&checked).unwrap();
}

#[test]
fn aliased_globals_keep_dependency_edges_for_inferred_private_callers() {
    let project = Project::new();
    project.write("model.mr", "pub fn value(x: Int) -> Int: x+1\n");
    let main = project.write(
        "main.mr",
        "import model as m\nfn caller(model): m.value(model)\nfn main(): println(caller(3))\n",
    );
    let loaded = modules::load(&main).unwrap();
    morrow_compiler::check::check(&loaded.program).unwrap();
}

#[test]
fn real_source_alias_shadowing_still_selects_local_record_fields() {
    let project = Project::new();
    project.write("model.mr", "pub fn value(x: Int) -> Int: x+1\n");
    let main=project.write("main.mr", "import model as m\ntype Callbacks:\n    value: fn(Int) -> Int\nfn main():\n    let m = Callbacks((x) -> x+2)\n    println(m.value(1))\n");
    let loaded = modules::load(&main).unwrap();
    morrow_compiler::check::check(&loaded.program).unwrap();
    let main = project.write(
        "main.mr",
        "import model as m\nfn main():\n    let m=3\n    println(m.value(1))\n",
    );
    let loaded = modules::load(&main).unwrap();
    assert!(morrow_compiler::check::check(&loaded.program).is_err());
}

#[test]
fn loader_preserves_source_spellings_and_resolved_declaration_identity() {
    use morrow_compiler::ast::{ExprKind, Stmt};
    let project = Project::new();
    project.write("model.mr", "pub fn value(x: Int) -> Int: x\n");
    let source = "import model as m\nfn main():\n    let model=3\n    let callable=m.value\n    let called=m.value(model)\n    let piped=model |> m.value()\n    println(called+piped)\n";
    let main = project.write("main.mr", source);
    let loaded = modules::load(&main).unwrap();
    let function = loaded
        .program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    let ExprKind::Block(statements) = &function.body.kind else {
        panic!()
    };
    for (statement, expected) in statements[1..4].iter().zip([0, 1, 2]) {
        let Stmt::Let { value, .. } = statement else {
            panic!()
        };
        let (name, resolved, kind) = match &value.kind {
            ExprKind::GlobalName { name, resolved } => (name, resolved, 0),
            ExprKind::GlobalCall { name, resolved, .. } => (name, resolved, 1),
            ExprKind::GlobalPipe {
                label: None,
                name,
                resolved,
                ..
            } => (name, resolved, 2),
            other => panic!("{other:?}"),
        };
        assert_eq!(name, "m.value");
        assert_eq!(resolved, "model.value");
        assert_eq!(kind, expected);
    }
    let formatted = morrow_compiler::format::format(source).unwrap();
    assert!(formatted.contains("m.value"));
    let main = project.write("main.mr", &formatted);
    morrow_compiler::check::check(&modules::load(&main).unwrap().program).unwrap();
}

#[test]
fn global_call_arguments_still_use_lexical_names_and_report_local_types() {
    let project = Project::new();
    project.write("model.mr", "pub fn value(x: Int) -> Int: x\n");
    for expr in [
        "m.value(model)",
        "model |> m.value()",
        "(() -> m.value(model))()",
    ] {
        let main = project.write(
            "main.mr",
            &format!(
                "import model as m\nfn main():\n    let model=\"wrong\"\n    println({expr})\n"
            ),
        );
        let loaded = modules::load(&main).unwrap();
        let error = morrow_compiler::check::check(&loaded.program).unwrap_err();
        assert!(error.message.contains("String"), "{error:?}");
        assert!(!error.message.contains("field access"), "{error:?}");
    }
}

#[test]
fn public_global_ast_nodes_validate_both_names_and_recursive_children() {
    use morrow_compiler::{
        Span,
        ast::{Expr, ExprKind},
        check, parse,
    };
    let span = Span::default();
    for kind in [
        ExprKind::GlobalName {
            name: "x".repeat(2 * 1024 * 1024),
            resolved: "main".into(),
        },
        ExprKind::GlobalCall {
            name: "x".into(),
            resolved: "x".repeat(2 * 1024 * 1024),
            args: vec![],
        },
        ExprKind::GlobalPipe {
            label: None,
            name: "x".into(),
            resolved: "main".into(),
            args: vec![],
            position: 0,
            value: Box::new(Expr {
                kind: ExprKind::String("x".repeat(2 * 1024 * 1024)),
                span,
            }),
        },
    ] {
        let mut program = parse::parse("fn main(): ()\n").unwrap();
        program.functions[0].body = Expr { kind, span };
        assert!(
            check::check(&program)
                .unwrap_err()
                .message
                .contains("limit")
        );
    }
}

#[test]
fn transparent_aliases_expand_inside_resolved_call_and_pipe_arguments() {
    let project = Project::new();
    project.write(
        "model.mr",
        "pub fn apply(f: fn(Int) -> Int, x: Int) -> Int: f(x)\n",
    );
    let main = project.write("main.mr", "import model as m\ntype Number = Int\nfn main():\n    let model=3\n    println(m.apply((x: Number) -> x+1, model))\n    println(model |> m.apply((x: Number) -> x+2, _))\n");
    let loaded = modules::load(&main).unwrap();
    let checked = morrow_compiler::check::check(&loaded.program).unwrap();
    morrow_compiler::lowering::emit(&checked).unwrap();
}
