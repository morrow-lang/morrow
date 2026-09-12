//! Bounded source type descriptions; inference ownership is never rewritten in error strings.
use crate::{Type, presentation};

/// Describe a checked type mismatch, retaining source quantifiers and distinct identities.
pub(super) fn mismatch(expected: &Type, actual: &Type) -> String {
    let mut budget = Budget {
        nodes: 4096,
        bytes: 65_536,
    };
    let pair = budget.ty(expected, 0).zip(budget.ty(actual, 0));
    let Some((left, right)) = pair else {
        return "types differ (details exceed the diagnostic limit)".into();
    };
    let left_text = description(&left);
    let right_text = description(&right);
    let distinct = if left == right && expected != actual {
        " (these are distinct type parameters)"
    } else {
        ""
    };
    format!("expected {left_text}, found {right_text}{distinct}")
}

/// Keep the known outer shape when unresolved children prevent a complete source signature.
fn description(ty: &Type) -> String {
    presentation::render_type(ty, presentation::Limits::default()).unwrap_or_else(|error| {
        if error.message.contains("limit") {
            return "a type whose details exceed the display limit".into();
        }
        match ty {
            Type::Function(_, _) => "Function with unresolved type variables".into(),
            Type::List(_) => "List with an unresolved element type".into(),
            Type::Option(_) => "Option with an unresolved payload type".into(),
            Type::Map(_, _) => "Map with unresolved key/value types".into(),
            Type::Result(_, _) => "Result with unresolved payload types".into(),
            Type::Tuple(_) => "Tuple with unresolved field types".into(),
            Type::Named(name, _) => format!("{name} with unresolved type arguments"),
            Type::Generic(_) => "a generic type".into(),
            _ => "an unresolved type".into(),
        }
    })
}

struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    /// Copy only bounded semantic type nodes, decoding owned names solely in Generic variants.
    fn ty(&mut self, ty: &Type, depth: usize) -> Option<Type> {
        if depth >= 128 {
            return None;
        }
        self.nodes = self.nodes.checked_sub(1)?;
        Some(match ty {
            Type::Generic(name) => Type::Generic(self.name(source_generic(name))?),
            Type::Named(name, args) => Type::Named(self.name(name)?, self.types(args, depth)?),
            Type::Union(args) => Type::Union(self.types(args, depth)?),
            Type::Tuple(args) => Type::Tuple(self.types(args, depth)?),
            Type::Function(args, result) => Type::Function(
                self.types(args, depth)?,
                Box::new(self.ty(result, depth + 1)?),
            ),
            Type::List(a) => Type::List(Box::new(self.ty(a, depth + 1)?)),
            Type::Option(a) => Type::Option(Box::new(self.ty(a, depth + 1)?)),
            Type::Map(a, b) => Type::Map(
                Box::new(self.ty(a, depth + 1)?),
                Box::new(self.ty(b, depth + 1)?),
            ),
            Type::Result(a, b) => Type::Result(
                Box::new(self.ty(a, depth + 1)?),
                Box::new(self.ty(b, depth + 1)?),
            ),
            _ => ty.clone(),
        })
    }
    /// Charge name bytes before allocation, including nominal names that must remain unchanged.
    fn name(&mut self, name: &str) -> Option<String> {
        self.bytes = self.bytes.checked_sub(name.len())?;
        Some(name.into())
    }
    /// Copy compound children under one aggregate diagnostic storage budget.
    fn types(&mut self, types: &[Type], depth: usize) -> Option<Vec<Type>> {
        types.iter().map(|ty| self.ty(ty, depth + 1)).collect()
    }
}

/// Decode only the exact compiler-owned rigid-variable form; all other names retain identity.
fn source_generic(name: &str) -> &str {
    let Some((owner, source)) = name.strip_prefix("$rigid").and_then(|s| s.split_once(':')) else {
        return name;
    };
    let mut chars = source.chars();
    let valid = chars.next().is_some_and(char::is_lowercase)
        && chars.all(|c| {
            c == '_' || c.is_ascii_alphanumeric() || (!c.is_ascii() && !c.is_whitespace())
        });
    if !owner.is_empty() && owner.bytes().all(|b| b.is_ascii_digit()) && valid {
        source
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_decoding_is_structural_and_does_not_conflate_owned_parameters() {
        let name = "$rigid12:a";
        let mut budget = Budget {
            nodes: 20,
            bytes: 100,
        };
        assert_eq!(
            budget.ty(&Type::Named(name.into(), vec![]), 0),
            Some(Type::Named(name.into(), vec![]))
        );
        assert_eq!(source_generic("$rigidx:a"), "$rigidx:a");
        assert_eq!(source_generic("$rigid1:a.bad"), "$rigid1:a.bad");
        let error = mismatch(
            &Type::Generic("$rigid0:a".into()),
            &Type::Generic("$rigid1:a".into()),
        );
        assert!(error.contains("distinct type parameters"));
        assert!(!error.contains("$rigid"));
    }
    #[test]
    fn diagnostic_name_storage_is_bounded_before_copying() {
        let error = mismatch(&Type::Generic("a".repeat(65_537)), &Type::Int);
        assert!(error.contains("diagnostic limit"));
        let error = mismatch(&Type::Tuple(vec![Type::Int; 3000]), &Type::Int);
        assert!(error.contains("display limit"), "{error}");
    }
}
