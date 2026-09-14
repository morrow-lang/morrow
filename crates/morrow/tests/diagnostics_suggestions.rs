//! Unresolved names, fields, builtin members and incomplete matches name the nearest fix.
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
            "morrow-suggest-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Resolve modules and check; the first diagnostic message from either stage is returned.
fn failure(source: &str) -> String {
    let project = Project::new();
    let main = project.write("main.fn", source);
    match modules::load(&main) {
        Err(error) => error.message,
        Ok(loaded) => morrow_compiler::check::check(&loaded.program)
            .err()
            .map(|d| d.message)
            .unwrap_or_else(|| panic!("source unexpectedly checked: {source}")),
    }
}

fn expect(source: &str, fragments: &[&str]) {
    let message = failure(source);
    for fragment in fragments {
        assert!(
            message.contains(fragment),
            "{message:?} does not contain {fragment:?}"
        );
    }
}

#[test]
fn misspelled_local_suggests_binding_in_scope() {
    expect(
        "fn main():\n    let count = 3\n    println(cuont)\n",
        &["unknown name 'cuont'", "did you mean 'count'?"],
    );
}

#[test]
fn misspelled_function_suggests_declared_function() {
    expect(
        "fn helper(x: Int) -> Int:\n    x * 2\n\nfn main():\n    println(helpr(2))\n",
        &["unknown function 'helpr'", "did you mean 'helper'?"],
    );
}

#[test]
fn misspelled_constructor_suggests_declared_constructor() {
    expect(
        "type Color:\n    Red\n    Green\n\nfn main():\n    let c = Gren\n    println(1)\n",
        &["unknown name 'Gren'", "did you mean 'Green'?"],
    );
}

#[test]
fn unknown_record_field_suggests_declared_field() {
    expect(
        "type Point:\n    x: Int\n    total: Int\n\nfn main():\n    let p = Point(1, 2)\n    println(p.totl)\n",
        &["unknown record field 'totl'", "did you mean 'total'?"],
    );
}

#[test]
fn unknown_builtin_member_names_the_module_and_nearest_api() {
    expect(
        "fn main():\n    println(String.upper(\"a\"))\n",
        &[
            "String has no function 'upper'",
            "did you mean 'String.to_upper'?",
        ],
    );
    expect(
        "fn main():\n    let m = Map.insert(Map.new(), \"a\", 1)\n    println(Map.len(m))\n",
        &["Map has no function 'insert'", "did you mean 'Map.put'?"],
    );
    expect(
        "fn main():\n    println(List.lenght([1]))\n",
        &["List has no function 'lenght'", "did you mean 'List.len'?"],
    );
}

#[test]
fn unknown_builtin_member_without_close_match_omits_suggestion() {
    let message = failure("fn main():\n    println(String.zzzzzzzzzz(\"a\"))\n");
    assert!(
        message.contains("String has no function 'zzzzzzzzzz'"),
        "{message}"
    );
    assert!(!message.contains("did you mean"), "{message}");
}

#[test]
fn unknown_user_module_member_keeps_import_guidance() {
    let message = failure("fn main():\n    println(widgets.render(1))\n");
    assert!(
        message.contains("widgets.render is private, not exported, or not imported"),
        "{message}"
    );
}

#[test]
fn distant_names_receive_no_suggestion() {
    let message = failure("fn main():\n    let count = 3\n    println(zzzzzzzzzz)\n");
    assert!(message.contains("unknown name 'zzzzzzzzzz'"), "{message}");
    assert!(!message.contains("did you mean"), "{message}");
}

#[test]
fn unknown_type_suggests_declared_or_builtin_type() {
    expect(
        "type Point:\n    x: Int\n\nfn origin() -> Poit:\n    Point(0)\n\nfn main():\n    println(origin().x)\n",
        &["unknown type 'Poit'", "did you mean 'Point'?"],
    );
    expect(
        "fn f(x: Strng) -> Int:\n    1\n\nfn main():\n    println(f(\"a\"))\n",
        &["unknown type 'Strng'", "did you mean 'String'?"],
    );
}

#[test]
fn incomplete_match_lists_missing_constructors() {
    expect(
        "type Color:\n    Red\n    Green\n    Blue\n\nfn name(c: Color) -> String:\n    match c:\n        Red -> \"red\"\n\nfn main():\n    println(name(Blue))\n",
        &["match must be exhaustive", "missing: Green, Blue"],
    );
    expect(
        "fn main():\n    let value = Some(3)\n    match value:\n        Some(n) -> println(n)\n",
        &["match must be exhaustive", "missing: None"],
    );
    expect(
        "type Shape:\n    Circle(radius: Int)\n    Rect(w: Int, h: Int)\n\nfn area(s: Shape) -> Int:\n    match s:\n        Circle(r) -> r\n\nfn main():\n    println(area(Circle(1)))\n",
        &["missing: Rect(_, _)"],
    );
}

#[test]
fn parser_names_the_unexpected_token_instead_of_a_prototype() {
    expect(
        "fn main():\n    let s = \"a\" ++ \"b\"\n    println(s)\n",
        &["expected an expression but found '+'"],
    );
    expect(
        "fn 3():\n    1\n\nfn main():\n    println(1)\n",
        &["expected an identifier but found number '3'"],
    );
    expect(
        "fn main():\n    let match = 1\n    println(1)\n",
        &["expected an identifier but found keyword 'match'"],
    );
    let message = failure("fn main():\n    let s = \"a\" ++ \"b\"\n");
    assert!(!message.contains("prototype"), "{message}");
}

#[test]
fn incomplete_match_over_open_scalars_asks_for_a_wildcard() {
    expect(
        "fn main():\n    let n = 3\n    let label = match n:\n        1 -> \"one\"\n    println(label)\n",
        &[
            "match must be exhaustive",
            "missing: a wildcard arm such as '_'",
        ],
    );
}

#[test]
fn incomplete_match_over_bool_and_result_names_literal_cases() {
    expect(
        "fn main():\n    let flag = true\n    match flag:\n        true -> println(1)\n",
        &["missing: false"],
    );
    expect(
        "fn main():\n    let r: Result(Int, String) = Ok(1)\n    match r:\n        Ok(n) -> println(n)\n",
        &["missing: Err(_)"],
    );
}
