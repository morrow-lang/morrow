//! Flat unconstrained columns spend work budget without consuming structural depth.
use super::*;

/// Charge actual inspected/copied matrix cells before doing their associated work.
fn charge(work: usize, budget: &mut usize) -> Lowering<()> {
    *budget = budget
        .checked_sub(work)
        .ok_or_else(|| invalid(Span::default(), "match coverage limit exceeded"))?;
    Ok(())
}

/// Validate matrix widths and recognize a covering row with bounded wildcard scanning.
pub(super) fn complete_row(
    rows: &[Vec<Pattern>],
    width: usize,
    budget: &mut usize,
) -> Lowering<bool> {
    for row in rows {
        charge(1, budget)?;
        if row.len() != width {
            return Err(invalid(Span::default(), "invalid coverage matrix width"));
        }
        let mut complete = true;
        for pattern in row {
            charge(1, budget)?;
            if !catchall(pattern) {
                complete = false;
                break;
            }
        }
        if complete {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Count a shared wildcard prefix without interpreting product width as recursive nesting.
pub(super) fn shared_columns(
    rows: &[Vec<Pattern>],
    width: usize,
    budget: &mut usize,
) -> Lowering<usize> {
    for index in 0..width {
        for row in rows {
            charge(1, budget)?;
            let pattern = row
                .get(index)
                .ok_or_else(|| invalid(Span::default(), "invalid coverage matrix width"))?;
            if !catchall(pattern) {
                return Ok(index);
            }
        }
    }
    Ok(width)
}

/// Copy only the constrained suffix after charging all resulting cells against the same budget.
pub(super) fn trim_columns(
    rows: &[Vec<Pattern>],
    common: usize,
    budget: &mut usize,
) -> Lowering<Vec<Vec<Pattern>>> {
    let mut trimmed = Vec::new();
    for row in rows {
        let suffix = row
            .get(common..)
            .ok_or_else(|| invalid(Span::default(), "invalid coverage matrix width"))?;
        charge(suffix.len(), budget)?;
        trimmed.push(suffix.to_vec());
    }
    Ok(trimmed)
}
