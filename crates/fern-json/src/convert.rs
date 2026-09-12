//! Exact decimal integers and explicitly rounded binary64 conversions.
use super::*;
struct Decimal {
    negative: bool,
    first: Option<usize>,
    last: usize,
    digits: usize,
    fraction: usize,
    exponent: i64,
}
/// Analyze validated number text without allocation, retaining significant positions and a capped exponent.
fn decimal(text: &str) -> Decimal {
    let mut shape = Decimal {
        negative: text.starts_with('-'),
        first: None,
        last: 0,
        digits: 0,
        fraction: 0,
        exponent: 0,
    };
    let mut fraction = false;
    for (i, byte) in text.bytes().enumerate().skip(usize::from(shape.negative)) {
        if matches!(byte, b'e' | b'E') {
            shape.exponent = exponent(&text[i + 1..]);
            break;
        }
        if byte == b'.' {
            fraction = true;
            continue;
        }
        if byte != b'0' {
            if shape.first.is_none() {
                shape.first = Some(shape.digits);
            }
            shape.last = shape.digits;
        }
        shape.digits += 1;
        shape.fraction += usize::from(fraction);
    }
    shape
}
/// Read validated exponent digits into a saturated signed value; magnitude never controls loop counts.
fn exponent(text: &str) -> i64 {
    let negative = text.starts_with('-');
    let cap = INPUT as i64 + 100;
    let mut value = 0;
    for byte in text.bytes().skip(usize::from(text.starts_with(['+', '-']))) {
        value = (value * 10 + i64::from(byte - b'0')).min(cap);
    }
    if negative { -value } else { value }
}
/// Convert a validated number lexeme exactly to signed64, distinguishing fractions from range errors.
pub fn integer(text: &str) -> Result<i64> {
    let shape = decimal(text);
    let Some(first) = shape.first else {
        return Ok(0);
    };
    let scale = shape.exponent - shape.fraction as i64 + (shape.digits - shape.last - 1) as i64;
    if scale < 0 {
        return Err(error(9, -1));
    }
    let significant = shape.last - first + 1;
    if significant > 19 || scale > 19 - significant as i64 {
        return Err(error(8, -1));
    }
    let mut magnitude = 0u64;
    for byte in text
        .bytes()
        .skip(usize::from(shape.negative))
        .filter(|b| *b != b'.')
        .skip(first)
        .take(significant)
    {
        magnitude = magnitude * 10 + u64::from(byte - b'0');
    }
    for _ in 0..scale {
        magnitude *= 10;
    }
    let maximum = i64::MAX as u64 + u64::from(shape.negative);
    if magnitude > maximum {
        return Err(error(8, -1));
    }
    Ok(if magnitude == 1 << 63 {
        i64::MIN
    } else if shape.negative {
        -(magnitude as i64)
    } else {
        magnitude as i64
    })
}
/// Round validated decimal text once to binary64; reject overflow and nonzero underflow while preserving -0.
pub fn float(text: &str) -> Result<f64> {
    let value: f64 = text.parse().map_err(|_| error(1, -1))?;
    if !value.is_finite() || (value == 0.0 && decimal(text).first.is_some()) {
        return Err(error(8, -1));
    }
    Ok(value)
}
/// Match C %.17g using exact-precision standard formatting, never shortest display.
pub fn format(value: f64) -> Result<String> {
    if !value.is_finite() {
        return Err(error(11, -1));
    }
    if value == 0.0 {
        return Ok(if value.is_sign_negative() { "-0" } else { "0" }.into());
    }
    let scientific = format!("{:.16e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').ok_or_else(|| error(8, -1))?;
    let exponent: i32 = exponent.parse().map_err(|_| error(8, -1))?;
    let mut digits = mantissa.replace('.', "");
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let mut output = if value.is_sign_negative() {
        "-".into()
    } else {
        String::new()
    };
    if !(-4..17).contains(&exponent) {
        output.push_str(&digits[..1]);
        if digits.len() > 1 {
            output.push('.');
            output.push_str(&digits[1..]);
        }
        output.push_str(&format!("e{exponent:+03}"));
    } else {
        fixed(&digits, exponent, &mut output);
    }
    Ok(output)
}
/// Append already-rounded digits in fixed form; `exponent` is restricted to the native %.17g fixed range.
fn fixed(digits: &str, exponent: i32, output: &mut String) {
    let point = exponent + 1;
    if point <= 0 {
        output.push_str("0.");
        for _ in 0..-point {
            output.push('0');
        }
        output.push_str(digits);
    } else if point as usize >= digits.len() {
        output.push_str(digits);
        for _ in digits.len()..point as usize {
            output.push('0');
        }
    } else {
        output.push_str(&digits[..point as usize]);
        output.push('.');
        output.push_str(&digits[point as usize..]);
    }
}
