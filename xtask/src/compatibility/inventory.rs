//! Validated snapshots preserve original acceptance data without Python execution.
use serde_json::Value;
use std::collections::BTreeSet;

pub fn native() -> Result<Vec<Value>, String> {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/compatibility-native.json"
    ))
    .map_err(|e| e.to_string())?;
    validate(&cases, 63)?;
    Ok(cases)
}
pub fn invalid() -> Result<Vec<Value>, String> {
    let mut cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/compatibility-invalid.json"
    ))
    .map_err(|e| e.to_string())?;
    validate(&cases, 160)?;
    for (group, text, count, preflight) in [
        (
            "actors",
            include_str!("../../../crates/fern/tests/actors/invalid.json"),
            16,
            Some("parse"),
        ),
        (
            "unions_native",
            include_str!("../../../crates/fern/tests/unions_native/invalid.json"),
            23,
            Some("fmt"),
        ),
        (
            "newtypes_native",
            include_str!("../../../crates/fern/tests/newtypes_native/invalid.json"),
            14,
            None,
        ),
        (
            "json_codecs_native",
            include_str!("../../../crates/fern/tests/json_codecs_native/invalid.json"),
            28,
            None,
        ),
        (
            "json_unions_native",
            include_str!("../../../crates/fern/tests/json_unions_native/invalid.json"),
            10,
            None,
        ),
    ] {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let values = if let Some(array) = value.as_array() {
            array.clone()
        } else {
            value
                .as_object()
                .ok_or("invalid rejection fixture")?
                .iter()
                .map(|(name, source)| serde_json::json!({"name":name,"source":source}))
                .collect()
        };
        validate(&values, count)?;
        for mut value in values {
            value["name"] = format!(
                "{group}/{}",
                value["name"].as_str().ok_or("missing rejection name")?
            )
            .into();
            if let Some(action) = preflight {
                value["preflight"] = action.into();
            }
            cases.push(value);
        }
    }
    validate(&cases, 251)?;
    Ok(cases)
}
fn validate(cases: &[Value], expected: usize) -> Result<(), String> {
    if cases.len() != expected {
        return Err(format!(
            "expected {expected} fixtures, found {}",
            cases.len()
        ));
    }
    let mut names = BTreeSet::new();
    for case in cases {
        let name = case["name"].as_str().ok_or("fixture name must be text")?;
        if name.is_empty() || !names.insert(name) || case["source"].as_str().is_none() {
            return Err("invalid or duplicate fixture".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_preserves_all_semantic_and_script_rejections() {
        let cases = invalid().unwrap();
        assert_eq!(cases.len(), 251);
        assert_eq!(
            cases
                .iter()
                .filter(|case| case["name"].as_str().unwrap().starts_with("test_rust_"))
                .count(),
            151
        );
        for group in [
            "actors",
            "unions_native",
            "newtypes_native",
            "json_codecs_native",
            "json_unions_native",
        ] {
            assert!(
                cases.iter().any(|case| case["name"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("{group}/"))),
                "{group}"
            );
        }
        assert_eq!(native().unwrap().len(), 63);
    }
    #[test]
    fn invalid_snapshots_cannot_silently_drop_or_duplicate_cases() {
        let case = serde_json::json!({"name":"case","source":"fn main():()"});
        assert!(validate(&[], 1).is_err());
        assert!(validate(&[case.clone(), case], 2).is_err());
        assert!(validate(&[serde_json::json!({"name":"case","source":42})], 1).is_err());
    }
}
