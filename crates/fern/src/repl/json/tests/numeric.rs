//! Checksums generated independently by scripts/json_repl_oracles.py using Decimal.
use super::*;
fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}
fn hash(hash: &mut u64, text: &str) {
    for byte in text.bytes() {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
}
#[test]
fn six_thousand_exact_decimal_and_binary64_oracles() {
    let mut state = 77;
    let mut digest = 14695981039346656037;
    for _ in 0..6000 {
        let sign = if next(&mut state) >> 63 != 0 { "-" } else { "" };
        let whole = next(&mut state);
        let fraction = next(&mut state);
        let exponent = (next(&mut state) % 801) as i64 - 400;
        let text = format!("{sign}{whole}.{fraction:020}e{exponent}");
        let int = match convert::integer(&text) {
            Ok(v) => format!("ok:{v}"),
            Err(e) => format!("err:{}", e.code),
        };
        let float = match convert::float(&text) {
            Ok(v) => format!("ok:{:016x}", v.to_bits()),
            Err(e) => format!("err:{}", e.code),
        };
        hash(&mut digest, &format!("{int}|{float}\n"));
    }
    assert_eq!(digest, 0x324e7a4b814e6448);
}
#[test]
fn six_thousand_binary64_builder_spellings_match_independent_oracle() {
    let mut state = 75;
    let mut digest = 14695981039346656037;
    for _ in 0..6000 {
        let value = f64::from_bits(next(&mut state));
        let text = convert::format(value).unwrap_or_else(|e| format!("err:{}", e.code));
        hash(&mut digest, &format!("{text}\n"));
    }
    assert_eq!(digest, 0x94597ff07428ab24);
}
#[test]
fn fixed_scientific_boundary_carry_and_signed_zero_spellings() {
    for (bits, expected) in [
        (0, "0"),
        (1 << 63, "-0"),
        (0x3f1a36e2eb1c432d, "0.0001"),
        (0x3ee4f8b588e368f1, "1.0000000000000001e-05"),
        (0x4341c37937e08000, "10000000000000000"),
        (0x4376345785d8a000, "1e+17"),
        (0x3f1a36e2eb1c432c, "9.9999999999999991e-05"),
        (0x4376345785d89fff, "99999999999999984"),
        (0x3ff0000000000001, "1.0000000000000002"),
        (0x0010000000000000, "2.2250738585072014e-308"),
        (1, "4.9406564584124654e-324"),
        (0x7fefffffffffffff, "1.7976931348623157e+308"),
    ] {
        assert_eq!(convert::format(f64::from_bits(bits)).unwrap(), expected);
    }
}
#[test]
fn halfway_rounding_subnormals_and_saturated_exponents() {
    for (text, bits) in [
        (
            "1.00000000000000011102230246251565404236316680908203125",
            0x3ff0000000000000,
        ),
        (
            "1.00000000000000011102230246251565404236316680908203126",
            0x3ff0000000000001,
        ),
        (
            "1.00000000000000033306690738754696212708950042724609375",
            0x3ff0000000000002,
        ),
        ("-0e99999999999999999999999999999", 1 << 63),
        ("4.9406564584124654e-324", 1),
    ] {
        assert_eq!(convert::float(text).unwrap().to_bits(), bits);
    }
    for text in [
        "1e999999999999999999999999",
        "1e-999999999999999999999999",
        "1e-324",
    ] {
        assert_eq!(convert::float(text).unwrap_err(), error(8, -1));
    }
}
