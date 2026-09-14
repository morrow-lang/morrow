//! Safe interactive counterparts for audited pure operations and local file calls.
use super::*;
const MAX_STRING: usize = 1024 * 1024;
const MAX_LIST: usize = 65_536;

impl Machine {
    /// Apply checked builtins to immutable shared values, bounding allocations first.
    pub(super) fn builtin(&mut self, builtin: ir::Builtin, args: Vec<Value>) -> Eval<Value> {
        use ir::Builtin::*;
        if self.comptime && matches!(builtin, Print | Println) {
            return Err(fault("output is not available during comptime evaluation"));
        }
        match (builtin, args.as_slice()) {
            (Print | Println, [value]) => {
                let text = match value {
                    Value::String(s) => s.to_string(),
                    other => display(other),
                };
                let extra = usize::from(builtin == Println);
                bounded(
                    self.output
                        .len()
                        .checked_add(text.len())
                        .and_then(|n| n.checked_add(extra)),
                    MAX_STRING,
                    "output",
                )?;
                self.output.push_str(&text);
                if builtin == Println {
                    self.output.push('\n');
                }
                Ok(Value::Unit)
            }
            (StringConcat, [Value::String(a), Value::String(b)]) => concat_strings(a, b),
            (StringEq, [Value::String(a), Value::String(b)]) => Ok(Value::Bool(a == b)),
            (StringLen, [Value::String(s)]) => Ok(Value::Int(s.len() as i64)),
            (ListLen, [Value::List(xs)]) => Ok(Value::Int(xs.len() as i64)),
            (ListIsEmpty, [Value::List(xs)]) => Ok(Value::Bool(xs.is_empty())),
            (ListGet, [Value::List(xs), Value::Int(i)]) => usize::try_from(*i)
                .ok()
                .and_then(|i| xs.get(i))
                .cloned()
                .ok_or_else(|| fault("list index out of bounds")),
            (ListHead, [Value::List(xs)]) => xs
                .first()
                .cloned()
                .ok_or_else(|| fault("head of empty list")),
            (ListTail, [Value::List(xs)]) => {
                list(xs.len().saturating_sub(1), xs.iter().skip(1).cloned())
            }
            (ListReverse, [Value::List(xs)]) => list(xs.len(), xs.iter().rev().cloned()),
            (ListPush, [Value::List(xs), value]) => list(
                xs.len() + 1,
                xs.iter().cloned().chain(std::iter::once(value.clone())),
            ),
            (ListConcat, [Value::List(a), Value::List(b)]) => {
                list(a.len() + b.len(), a.iter().chain(b.iter()).cloned())
            }
            (ListContains, [Value::List(xs), value]) => Ok(Value::Bool(xs.contains(value))),
            (OptionIsSome | ResultIsOk, [Value::Sum(tag, _)]) => Ok(Value::Bool(*tag == 0)),
            (OptionIsNone | ResultIsErr, [Value::Sum(tag, _)]) => Ok(Value::Bool(*tag == 1)),
            (OptionUnwrapOr | ResultUnwrapOr, [Value::Sum(tag, fields), fallback]) => {
                Ok(if *tag == 0 {
                    fields
                        .first()
                        .cloned()
                        .ok_or_else(|| fault("missing sum payload"))?
                } else {
                    fallback.clone()
                })
            }
            _ => self.higher_order(builtin, &args),
        }
    }
    /// Dispatch stable registry identities and name unsupported operations by their source API.
    pub(super) fn runtime(&mut self, id: usize, args: Vec<Value>) -> Eval<Value> {
        let signature = runtime::signature(id).ok_or_else(|| fault("unknown runtime function"))?;
        if matches!(
            signature.symbol,
            "fern_ffi_borrow_string" | "fern_ffi_read_string"
        ) {
            return Err(fault("foreign pointer operations require native execution"));
        }
        if signature.symbol == "fern_ffi_float32" {
            let [Value::Float(value)] = args.as_slice() else {
                return Err(fault("invalid CFloat32 conversion"));
            };
            let narrow = *value as f32;
            return if value.is_finite() && !narrow.is_finite() {
                Ok(Value::Sum(
                    1,
                    vec![Value::String(Rc::new(
                        "foreign scalar conversion is outside its representable range".into(),
                    ))]
                    .into(),
                ))
            } else {
                Ok(Value::Sum(0, vec![Value::Float(f64::from(narrow))].into()))
            };
        }
        if let Some(builtin) = core_builtin(signature.symbol, signature.operation) {
            return self.builtin(builtin, args);
        }
        if let Some(value) = self.json(signature.symbol, &args) {
            return value;
        }
        if signature.operation == runtime::Operation::DecimalPredicate {
            let [Value::String(text)] = args.as_slice() else {
                return Err(fault("invalid decimal predicate arguments"));
            };
            if text.len() > 16 * 1024 * 1024 {
                return Err(fault("string size limit exceeded"));
            }
            self.charge_steps(text.len().div_ceil(64).max(1))?;
            return decimal(text);
        }
        if let Some(value) = strings(signature.symbol, &args) {
            return value;
        }
        if let Some(value) = positional(signature.symbol, &args) {
            return value;
        }
        if self.comptime {
            return Err(fault(
                "this host API is not available during comptime evaluation",
            ));
        }
        if self.simulation {
            return Err(fault("host APIs are not available during actor simulation"));
        }
        if matches!(
            signature.symbol,
            "fern_read_file"
                | "fern_write_file"
                | "fern_append_file"
                | "fern_delete_file"
                | "fern_file_size"
                | "fern_file_exists"
                | "fern_is_dir"
        ) {
            return files(signature.symbol, &args);
        }
        let name = runtime::names()
            .into_iter()
            .find(|name| runtime::resolve(name) == Some(id))
            .unwrap_or("requested API");
        Err(fault(format!(
            "{name} is available in native programs; interactive support is not implemented"
        )))
    }
}

/// Map native aliases to the same semantic implementation, including inverted predicates.
fn core_builtin(symbol: &str, operation: runtime::Operation) -> Option<ir::Builtin> {
    use ir::Builtin::*;
    Some(match symbol {
        "fern_str_concat" => StringConcat,
        "fern_str_eq" => StringEq,
        "fern_str_len" => StringLen,
        "fern_list_len" => ListLen,
        "fern_list_get" => ListGet,
        "fern_list_head" => ListHead,
        "fern_list_tail" => ListTail,
        "fern_list_push" => ListPush,
        "fern_list_reverse" => ListReverse,
        "fern_list_concat" => ListConcat,
        "fern_list_is_empty" => ListIsEmpty,
        "fern_list_contains" => ListContains,
        "fern_result_is_ok" if operation == runtime::Operation::InvertBool => ResultIsErr,
        "fern_result_is_ok" => ResultIsOk,
        "fern_result_unwrap_or" => ResultUnwrapOr,
        _ => return None,
    })
}

/// Check arithmetic and allocation budgets before constructing the backing storage.
fn bounded(size: Option<usize>, maximum: usize, kind: &str) -> Eval<usize> {
    size.filter(|size| *size <= maximum)
        .ok_or_else(|| fault(format!("interactive {kind} limit exceeded")))
}
fn string(value: &str) -> Eval<Value> {
    bounded(Some(value.len()), MAX_STRING, "string")?;
    Ok(Value::String(Rc::new(value.into())))
}
fn list(length: usize, values: impl Iterator<Item = Value>) -> Eval<Value> {
    bounded(Some(length), MAX_LIST, "list")?;
    Ok(Value::List(Rc::new(values.collect())))
}
/// Both source concatenation spellings share the same preallocation bound.
pub(super) fn concat_strings(a: &str, b: &str) -> Eval<Value> {
    let length = bounded(a.len().checked_add(b.len()), MAX_STRING, "string")?;
    let mut text = String::with_capacity(length);
    text.push_str(a);
    text.push_str(b);
    Ok(Value::String(Rc::new(text)))
}
fn sum(tag: usize, value: Value) -> Value {
    Value::Sum(tag, Rc::new(vec![value]))
}
fn file_result(value: Result<Value, i64>) -> Value {
    match value {
        Ok(value) => sum(0, value),
        Err(code) => sum(1, Value::Int(code)),
    }
}
fn option(value: Option<usize>) -> Value {
    value.map_or_else(
        || Value::Sum(1, Rc::new(Vec::new())),
        |n| sum(0, Value::Int(n as i64)),
    )
}
fn optional(value: Option<Value>) -> Value {
    value.map_or_else(|| Value::Sum(1, Rc::new(Vec::new())), |value| sum(0, value))
}

fn checked(value: Option<i64>) -> Value {
    optional(value.map(Value::Int))
}
/// Mirror the native element-directed sort: Int/Bool numeric, Float total order, String bytes.
fn order(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
        (Value::String(a), Value::String(b)) => a.as_bytes().cmp(b.as_bytes()),
        _ => std::cmp::Ordering::Equal,
    }
}

/// Non-faulting positional access mirrors the native clamping and `None` contracts exactly.
fn positional(symbol: &str, args: &[Value]) -> Option<Eval<Value>> {
    let clamp = |count: &i64, len: usize| usize::try_from(*count).map_or(0, |n| n.min(len));
    Some(match (symbol, args) {
        ("fern_list_at", [Value::List(xs), Value::Int(i)]) => Ok(optional(
            usize::try_from(*i).ok().and_then(|i| xs.get(i)).cloned(),
        )),
        ("fern_list_first", [Value::List(xs)]) => Ok(optional(xs.first().cloned())),
        ("fern_list_last", [Value::List(xs)]) => Ok(optional(xs.last().cloned())),
        ("fern_list_take", [Value::List(xs), Value::Int(n)]) => {
            let count = clamp(n, xs.len());
            list(count, xs.iter().take(count).cloned())
        }
        ("fern_list_drop", [Value::List(xs), Value::Int(n)]) => {
            let count = clamp(n, xs.len());
            list(xs.len() - count, xs.iter().skip(count).cloned())
        }
        ("fern_list_sum", [Value::List(xs)]) => {
            Ok(Value::Int(xs.iter().fold(0, |total, x| match x {
                Value::Int(n) => total.wrapping_add(*n),
                _ => total,
            })))
        }
        ("fern_list_range", [Value::Int(start), Value::Int(end)]) => {
            match usize::try_from((i128::from(*end) - i128::from(*start)).max(0)) {
                Ok(count) => list(
                    count,
                    (0..count).map(|offset| Value::Int(start + offset as i64)),
                ),
                Err(_) => Err(fault("interactive list limit exceeded")),
            }
        }
        ("fern_list_zip", [Value::List(a), Value::List(b)]) => list(
            a.len().min(b.len()),
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| Value::Sum(0, Rc::new(vec![x.clone(), y.clone()]))),
        ),
        ("fern_list_sort", [Value::List(xs)]) => {
            let mut sorted = xs.as_ref().clone();
            sorted.sort_by(order);
            list(sorted.len(), sorted.into_iter())
        }
        ("fern_int_checked_add", [Value::Int(a), Value::Int(b)]) => Ok(checked(a.checked_add(*b))),
        ("fern_int_checked_sub", [Value::Int(a), Value::Int(b)]) => Ok(checked(a.checked_sub(*b))),
        ("fern_int_checked_mul", [Value::Int(a), Value::Int(b)]) => Ok(checked(a.checked_mul(*b))),
        ("fern_int_checked_div", [Value::Int(a), Value::Int(b)]) => Ok(checked(a.checked_div(*b))),
        ("fern_int_checked_rem", [Value::Int(a), Value::Int(b)]) => Ok(checked(a.checked_rem(*b))),
        ("fern_int_checked_neg", [Value::Int(a)]) => Ok(checked(a.checked_neg())),
        ("fern_int_parse", [Value::String(text)]) => {
            let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
            let exact = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            Ok(optional(
                exact
                    .then(|| text.parse::<i64>().ok())
                    .flatten()
                    .map(Value::Int),
            ))
        }
        _ => return None,
    })
}

/// Preserve the runtime's ASCII case conversion and exact four-character trim set.
fn strings(symbol: &str, args: &[Value]) -> Option<Eval<Value>> {
    let whitespace = |c| matches!(c, ' ' | '\t' | '\n' | '\r');
    Some(match (symbol, args) {
        ("fern_str_compare", [Value::String(a), Value::String(b)]) => {
            Ok(Value::Int(match a.cmp(b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }))
        }
        ("fern_str_to_upper", [Value::String(s)]) => ascii_case(s, true),
        ("fern_str_to_lower", [Value::String(s)]) => ascii_case(s, false),
        ("fern_str_trim", [Value::String(s)]) => string(s.trim_matches(whitespace)),
        ("fern_str_trim_start", [Value::String(s)]) => string(s.trim_start_matches(whitespace)),
        ("fern_str_trim_end", [Value::String(s)]) => string(s.trim_end_matches(whitespace)),
        ("fern_str_contains", [Value::String(s), Value::String(p)]) => {
            Ok(Value::Bool(s.contains(p.as_str())))
        }
        ("fern_str_starts_with", [Value::String(s), Value::String(p)]) => {
            Ok(Value::Bool(s.starts_with(p.as_str())))
        }
        ("fern_str_ends_with", [Value::String(s), Value::String(p)]) => {
            Ok(Value::Bool(s.ends_with(p.as_str())))
        }
        ("fern_str_is_empty", [Value::String(s)]) => Ok(Value::Bool(s.is_empty())),
        ("fern_str_index_of", [Value::String(s), Value::String(p)]) => {
            Ok(option(s.find(p.as_str())))
        }
        ("fern_str_char_at", [Value::String(s), Value::Int(i)]) => Ok(option(
            usize::try_from(*i)
                .ok()
                .and_then(|i| s.as_bytes().get(i))
                .map(|b| *b as usize),
        )),
        ("fern_str_slice", [Value::String(s), Value::Int(start), Value::Int(end)]) => {
            slice(s, *start, *end)
        }
        ("fern_str_repeat", [Value::String(s), Value::Int(n)]) => repeat(s, *n),
        ("fern_str_replace", [Value::String(s), Value::String(old), Value::String(new)]) => {
            replace(s, old, new)
        }
        ("fern_str_split", [Value::String(s), Value::String(p)]) => split(s, p),
        ("fern_str_lines", [Value::String(s)]) => lines(s),
        ("fern_str_join", [Value::List(xs), Value::String(separator)]) => join(xs, separator),
        _ => return None,
    })
}

/// Case conversion preserves byte length and checks its allocation before copying.
fn ascii_case(text: &str, upper: bool) -> Eval<Value> {
    bounded(Some(text.len()), MAX_STRING, "string")?;
    let mut value = text.to_owned();
    if upper {
        value.make_ascii_uppercase();
    } else {
        value.make_ascii_lowercase();
    }
    Ok(Value::String(Rc::new(value)))
}

/// Native indices count bytes; unsupported invalid UTF-8 results are explicit, never lossy.
fn slice(text: &str, start: i64, end: i64) -> Eval<Value> {
    let start = start.max(0);
    let end = end.max(start);
    let start = (start as u64).min(text.len() as u64) as usize;
    let end = (end as u64).min(text.len() as u64) as usize;
    string(
        text.get(start..end)
            .ok_or_else(|| fault("String.slice indices must be UTF-8 character boundaries"))?,
    )
}
fn repeat(text: &str, count: i64) -> Eval<Value> {
    if count <= 0 || text.is_empty() {
        return string("");
    }
    if count as u64 > (16 * 1024 * 1024 / text.len()) as u64 {
        return Err(fault("string size limit exceeded"));
    }
    let count = usize::try_from(count).map_err(|_| fault("interactive string limit exceeded"))?;
    bounded(text.len().checked_mul(count), MAX_STRING, "string")?;
    Ok(Value::String(Rc::new(text.repeat(count))))
}
fn replace(text: &str, old: &str, new: &str) -> Eval<Value> {
    if old.is_empty() {
        return string(text);
    }
    let count = text.matches(old).count();
    let retained = text.len() - count * old.len();
    bounded(
        count
            .checked_mul(new.len())
            .and_then(|n| retained.checked_add(n)),
        MAX_STRING,
        "string",
    )?;
    Ok(Value::String(Rc::new(text.replace(old, new))))
}
fn split(text: &str, separator: &str) -> Eval<Value> {
    if separator.is_empty() {
        return list(
            text.chars().count(),
            text.chars()
                .map(|character| Value::String(Rc::new(character.to_string()))),
        );
    }
    let parts = text.split(separator);
    let length = parts.clone().count();
    list(
        length,
        parts.map(|part| Value::String(Rc::new(part.into()))),
    )
}
fn lines(text: &str) -> Eval<Value> {
    let mut values = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find(['\r', '\n']) {
        bounded(Some(values.len() + 1), MAX_LIST, "list")?;
        values.push(Value::String(Rc::new(rest[..index].into())));
        rest = &rest[index
            + if rest[index..].starts_with("\r\n") {
                2
            } else {
                1
            }..];
    }
    if !rest.is_empty() || values.is_empty() {
        bounded(Some(values.len() + 1), MAX_LIST, "list")?;
        values.push(Value::String(Rc::new(rest.into())));
    }
    Ok(Value::List(Rc::new(values)))
}
fn join(values: &[Value], separator: &str) -> Eval<Value> {
    let mut length = bounded(
        separator.len().checked_mul(values.len().saturating_sub(1)),
        MAX_STRING,
        "string",
    )?;
    for value in values {
        let Value::String(text) = value else {
            return Err(fault("String.join requires strings"));
        };
        length = bounded(length.checked_add(text.len()), MAX_STRING, "string")?;
    }
    let mut text = String::with_capacity(length);
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            text.push_str(separator);
        }
        if let Value::String(value) = value {
            text.push_str(value);
        }
    }
    Ok(Value::String(Rc::new(text)))
}

/// Fern file errors are fixed public codes, never platform errno values.
fn files(symbol: &str, args: &[Value]) -> Eval<Value> {
    let Some(Value::String(path)) = args.first() else {
        return Err(fault("file path must be String"));
    };
    let path = std::path::Path::new(path.as_str());
    match symbol {
        "fern_file_exists" => Ok(Value::Bool(std::fs::File::open(path).is_ok())),
        "fern_is_dir" => Ok(Value::Bool(path.is_dir())),
        "fern_read_file" => read_file(path),
        "fern_file_size" => Ok(file_result(file_size(path).map(Value::Int))),
        "fern_write_file" | "fern_append_file" => {
            let Some(Value::String(text)) = args.get(1) else {
                return Err(fault("file contents must be String"));
            };
            Ok(file_result(
                write_file(path, text, symbol == "fern_append_file").map(Value::Int),
            ))
        }
        "fern_delete_file" => {
            let result = std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path));
            Ok(file_result(result.map(|()| Value::Int(0)).map_err(|_| 1)))
        }
        _ => Err(fault("unsupported file operation")),
    }
}
fn file_size(path: &std::path::Path) -> Result<i64, i64> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|_| 1)?;
    file.seek(SeekFrom::End(0))
        .map_err(|_| 3)
        .and_then(|n| i64::try_from(n).map_err(|_| 3))
}
/// Publish complete UTF8/NUL-free text within the native profile and stricter session budget.
/// Safe std File drop cannot observe a late OS close error; all explicit reads are checked.
fn read_file(path: &std::path::Path) -> Eval<Value> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return Ok(file_result(Err(1))),
    };
    let length = match file.seek(SeekFrom::End(0)) {
        Ok(n) if n <= 16 * 1024 * 1024 => n,
        _ => return Ok(file_result(Err(3))),
    };
    let length = bounded(usize::try_from(length).ok(), MAX_STRING, "string")?;
    if file.seek(SeekFrom::Start(0)).is_err() {
        return Ok(file_result(Err(3)));
    }
    let mut bytes = vec![0; length + 1];
    if file.read_exact(&mut bytes[..length]).is_err()
        || !matches!(file.read(&mut bytes[length..]), Ok(0))
    {
        return Ok(file_result(Err(3)));
    }
    bytes.truncate(length);
    if bytes.contains(&0) {
        return Ok(file_result(Err(3)));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(file_result(Err(3)));
    };
    Ok(file_result(Ok(Value::String(Rc::new(text)))))
}

/// Validate before target side effects, then complete unbuffered writes without claiming durability.
fn write_file(path: &std::path::Path, text: &str, append: bool) -> Result<i64, i64> {
    use std::io::Write;
    if text.len() > 16 * 1024 * 1024 || text.contains('\0') {
        return Err(3);
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|_| 2)?;
    file.write_all(text.as_bytes()).map_err(|_| 3)?;
    Ok(text.len() as i64)
}

#[cfg(test)]
mod file_text_tests {
    use super::write_file;

    #[test]
    fn write_preflight_preserves_existing_target_for_nul_and_oversize() {
        let path = std::env::temp_dir().join(format!("fern-file-preflight-{}", std::process::id()));
        std::fs::write(&path, b"original").unwrap();
        let oversized = "x".repeat(16 * 1024 * 1024 + 1);
        for append in [false, true] {
            for text in ["a\0b", oversized.as_str()] {
                assert_eq!(write_file(&path, text, append), Err(3));
                assert_eq!(std::fs::read(&path).unwrap(), b"original");
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unbuffered_file_errors_are_returned_before_success() {
        for append in [false, true] {
            assert_eq!(
                write_file(std::path::Path::new("/dev/full"), "lost", append),
                Err(3)
            );
        }
    }
}

/// Apply the native decimal content ceiling before scanning; retained interactive limits remain stricter.
fn decimal(text: &str) -> Eval<Value> {
    if text.len() > 16 * 1024 * 1024 {
        return Err(fault("string size limit exceeded"));
    }
    Ok(Value::Bool(crate::decimal::is_decimal(text)))
}

#[cfg(test)]
mod decimal_tests {
    use super::*;
    #[test]
    fn decimal_scan_reserves_work_before_scanning_and_keeps_cleanup_budget_separate() {
        let mut machine = Machine::new(Rc::new(ir::Program::default()), HashMap::new());
        let id = runtime::resolve("String.is_decimal").unwrap();
        machine.steps = 99_999;
        assert!(
            machine
                .runtime(id, vec![Value::String(Rc::new("1".repeat(128)))])
                .is_err()
        );
        assert!(machine.steps > 100_000);
        machine.cleanup_depth = 1;
        assert!(matches!(
            machine.runtime(id, vec![Value::String(Rc::new("1".into()))]),
            Ok(Value::Bool(true))
        ));
        assert_eq!(machine.cleanup_steps, 1);
    }
    #[test]
    fn oversized_decimal_is_a_fault_even_when_first_byte_is_not_decimal() {
        let mut text = "1".repeat(16 * 1024 * 1024 + 1);
        assert!(decimal(&text).is_err());
        text.replace_range(..1, "x");
        assert!(decimal(&text).is_err());
        text.pop();
        assert!(matches!(decimal(&text), Ok(Value::Bool(false))));
    }
}
