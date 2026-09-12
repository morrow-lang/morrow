//! Conservative shallow intersections never inspect record values or array elements.
use super::*;
/// Unknowns defer proof only; they do not suppress an overlapping concrete pair elsewhere.
pub(super) fn disjoint(
    a: &Shape<'_>,
    b: &Shape<'_>,
    work: &mut usize,
    span: Span,
) -> Result<bool, Diagnostic> {
    if matches!(a, Shape::Unknown) || matches!(b, Shape::Unknown) {
        return Ok(true);
    }
    if matches!(a, Shape::Any) || matches!(b, Shape::Any) {
        return Ok(false);
    }
    if category(a) != category(b) {
        return Ok(true);
    }
    match (a, b) {
        (Shape::Array(Some(a)), Shape::Array(Some(b))) => Ok(a != b),
        (Shape::Sum(a), Shape::Sum(b)) => {
            for a in a {
                for b in b {
                    if same(a, b, work, span)? {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        }
        (Shape::Object(a), Shape::Object(b)) => keys_disjoint(a, b, work, span),
        (Shape::Sum(_), Shape::Object(b)) | (Shape::Object(b), Shape::Sum(_)) => keys_disjoint(
            &[
                Key {
                    name: "tag",
                    required: true,
                },
                Key {
                    name: "fields",
                    required: true,
                },
            ],
            b,
            work,
            span,
        ),
        _ => Ok(false),
    }
}
fn category(shape: &Shape<'_>) -> u8 {
    match shape {
        Shape::Null => 0,
        Shape::Bool => 1,
        Shape::Number => 2,
        Shape::String => 3,
        Shape::Array(_) => 4,
        Shape::Object(_) | Shape::Sum(_) | Shape::Map => 5,
        Shape::Any | Shape::Unknown | Shape::Follow => 6,
    }
}
/// Required key forbidden by the other strict shape proves the intersection empty.
fn keys_disjoint(
    a: &[Key<'_>],
    b: &[Key<'_>],
    work: &mut usize,
    span: Span,
) -> Result<bool, Diagnostic> {
    for (left, right) in [(a, b), (b, a)] {
        for key in left {
            charge(work, 1, span)?;
            if !key.required {
                continue;
            }
            let mut allowed = false;
            for other in right {
                allowed |= same(key.name, other.name, work, span)?;
            }
            if !allowed {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn same(a: &str, b: &str, work: &mut usize, span: Span) -> Result<bool, Diagnostic> {
    charge(work, a.len().min(b.len()) + 1, span)?;
    Ok(a == b)
}
