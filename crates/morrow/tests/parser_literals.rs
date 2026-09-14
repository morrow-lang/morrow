use morrow_compiler::{format, parse};

#[test]
fn integer_bases_separators_and_bitwise_power_roundtrip() {
    for source in [
        "fn main():\n    println(0xff + 0X2A + 0b1010 + 0B10 + 0o77 + 0O10 + 1_000)\n    println(-0x8000_0000_0000_0000)\n",
        "fn main():\n    println(2 ** 3 ** 2)\n    println(-2**2)\n    println(~~~0 &&& 7 ||| 8 ^^^ 2)\n    println(1 <<< 3 >>> 1)\n",
        "fn main():\n    match 255:\n        0xFF -> println(1)\n        -0b10 -> println(2)\n        _ -> ()\n",
    ] {
        let canonical = format::format(source).unwrap();
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn invalid_base_digits_separators_and_magnitudes_are_rejected() {
    for literal in [
        "0x",
        "0b2",
        "0o8",
        "0x_1",
        "0x1_",
        "1__2",
        "0x8000000000000000",
        "-0x8000000000000001",
        "0b102",
        "0x1g",
    ] {
        let source = format!("fn main(): println({literal})\n");
        assert!(parse::parse(&source).is_err(), "{literal}");
    }
}
