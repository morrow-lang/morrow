use morrow_compiler::{check, format, lowering, parse};

#[test]
fn typed_clauses_guards_and_documentation_preserve_checked_emission() {
    for source in [
        "@doc \"\"\"Factorial.\"\"\"\nfn fact(0: Int) -> Int: 1\n# recursion\nfn fact(n: Int) -> Int: n*fact(n-1)\nfn main(): println(fact(5))\n",
        "fn size([]: List(Int)) -> 0\nfn size([_,..tail]: List(Int)) -> 1+size(tail)\nfn main(): println(size([1,2]))\n",
        "fn select((first,..tail): (Int,Int)) if first>0 -> Int: first\nfn select(_: (Int,Int)) -> Int: 0\nfn main(): println(select((1,2)))\n",
        "fn classify(x: Int) if ((n: Int) -> n>0)(x) -> 1\nfn classify(_: Int) ->\n    0\nfn main(): println(classify(1))\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(canonical, format::format(&canonical).unwrap());
        let before =
            lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap();
        let after =
            lowering::emit(&check::check(&parse::parse(&canonical).unwrap()).unwrap()).unwrap();
        assert_eq!(before, after);
    }
}
