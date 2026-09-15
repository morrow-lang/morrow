use morrow_compiler::lsp;

fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\u{1}', "\\u0001")
    )
}
fn run(messages: Vec<String>) -> Vec<String> {
    use std::fmt::Write;
    let mut input = String::new();
    for message in messages {
        write!(
            &mut input,
            "Content-Length: {}\r\n\r\n{message}",
            message.len()
        )
        .unwrap();
    }
    let mut output = Vec::new();
    lsp::serve(std::io::Cursor::new(input), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    text.split("Content-Length: ")
        .skip(1)
        .map(|frame| frame.split_once("\r\n\r\n").unwrap().1.to_owned())
        .collect()
}

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

fn query(project: &Project, marked: &str, method: &str) -> String {
    let offset = marked.find('§').unwrap();
    let before = &marked[..offset];
    let line = before.bytes().filter(|b| *b == b'\n').count();
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count();
    let source = marked.replace('§', "");
    let path = project.write("main.mr", &source);
    let uri = format!("file://{}", path.display());
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":{},"version":1,"text":{}}}}}}}"#,
            quote(&uri),
            quote(&source)
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":"query","method":"textDocument/{method}","params":{{"textDocument":{{"uri":{}}},"position":{{"line":{line},"character":{character}}}}}}}"#,
            quote(&uri)
        ),
        r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#.into(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.into(),
    ];
    run(messages)
        .into_iter()
        .find(|s| s.contains("\"id\":\"query\""))
        .unwrap()
}

#[test]
fn imported_union_narrowing_keeps_local_and_member_definition_offsets() {
    let project = Project::new();
    let model =
        "pub newtype Id=Wrapped(Int)\npub type Choice=Int | Id\npub fn Choice(x:Int)->Int:x\n";
    project.write("model.mr", model);
    project.write("api.mr", "pub import model.{Id,Choice}\n");
    let source = "import api as m\nfn size(x:m.Choice)->Int:\n    match x:\n        n:Int -> m.Choice(n)\n        id:m.Id -> id.0\nfn main():()\n";
    let local = query(
        &project,
        &source.replace("Choice(n)", "Choice(n§)"),
        "definition",
    );
    assert!(
        local.contains("main.mr") && local.contains("\"start\":{\"character\":8,\"line\":3}"),
        "{local}"
    );
    let member = query(&project, &source.replace("id.0", "id.0§"), "definition");
    assert!(member.contains("model.mr"), "{member}");
    assert!(
        member.contains(&format!(
            "\"start\":{{\"character\":{},\"line\":0}}",
            model.find("Int").unwrap()
        )),
        "{member}"
    );
    let ty = query(&project, &source.replace("id:m.Id", "id:m.Id§"), "hover");
    assert!(ty.contains("newtype") && ty.contains("Id"), "{ty}");
}
#[test]
fn private_typed_union_alternatives_produce_no_fabricated_editor_targets() {
    let project = Project::new();
    project.write(
        "model.mr",
        "newtype Secret=Hidden(Int)\npub type Choice=Int | Secret\n",
    );
    let source = "import model as m\nfn size(x:m.Choice)->Int:\n    match x:\n        n:Int -> n\n        hidden:m.Secret§ -> hidden.0\n";
    for method in ["hover", "definition"] {
        let response = query(&project, source, method);
        assert!(response.contains("\"result\":null"), "{response}");
    }
}
