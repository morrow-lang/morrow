use morrow_compiler::{check, format, lowering, parse};

#[test]
fn sequence_patterns_preserve_checked_emission_and_comments() {
    for source in [
        "fn length(items: List(Int)) -> Int:\n    match items:\n        [] -> 0\n        [head,..tail] -> 1+length(tail)\nfn main(): println(length([1,2,3]))\n",
        "fn main():\n    let (first,..tail)=(1,true,\"🌿\")\n    let (..whole)=(1,)\n    let (only,..empty)=(1,)\n    let [..items]=[1,2]\n    println(tail.1)\n    println(whole.0+only+List.len(items))\n",
        "fn load() -> Result(List(Int),String): Ok([1,2])\nfn read() -> Int:\n    with [..items] <- load() do List.len(items) else Err(_) -> 0\nfn main():\n    for (head,..tail) in [(1, true), (2,false)]: println(tail.0)\n    println(read())\n",
        "fn first(items: List(Int)) -> Int:\n    let [head,..tail]=items else: return 0\n    match tail:\n        [second,.._] if second>0 -> head+second # matched suffix\n        _ -> head\nfn main(): println(first([1,2]))\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
        let emit = |text: &str| {
            lowering::emit(&check::check(&parse::parse(text).unwrap()).unwrap()).unwrap()
        };
        assert_eq!(emit(source), emit(&canonical));
    }
}

#[test]
fn native_sequence_fixtures_preserve_canonical_syntax_and_checked_emission() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sequences");
    let mut paths = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mr"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        let canonical =
            format::format(&source).unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
        assert_eq!(
            format::format(&canonical).unwrap(),
            canonical,
            "{}",
            path.display()
        );
        let emit = |source: &str| {
            lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
        };
        assert_eq!(emit(&source), emit(&canonical), "{}", path.display());
    }
}
