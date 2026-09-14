//! Unicode16 decimal membership using checksum-generated tables, independent of the host Rust UCD.
use crate::decimal_table::RANGES;

/// Classify nonempty valid text; the caller must charge/check its input-byte ceiling first.
pub(crate) fn is_decimal(text: &str) -> bool {
    !text.is_empty() && text.chars().all(member)
}

/// Test one scalar against sorted, disjoint pinned Nd intervals without allocating.
fn member(scalar: char) -> bool {
    let value = scalar as u32;
    if value < 128 {
        return scalar.is_ascii_digit();
    }
    RANGES
        .binary_search_by(|(low, high)| {
            if value < *low {
                std::cmp::Ordering::Greater
            } else if value > *high {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_scalar_agrees_with_primary_ucd_categories() {
        let input =
            include_str!("../../../tests/fixtures/unicode/16.0.0/DerivedGeneralCategory.txt");
        let mut expected = vec![false; 0x110000];
        for line in input.lines() {
            let text = line.split('#').next().unwrap().trim();
            let Some((span, category)) = text.split_once(';') else {
                continue;
            };
            if category.trim() != "Nd" {
                continue;
            }
            let ends: Vec<_> = span
                .trim()
                .split("..")
                .map(|s| usize::from_str_radix(s, 16).unwrap())
                .collect();
            for item in &mut expected[ends[0]..=ends[ends.len() - 1]] {
                *item = true;
            }
        }
        for (code, expected) in expected.into_iter().enumerate() {
            if let Some(scalar) = char::from_u32(code as u32) {
                assert_eq!(member(scalar), expected, "U+{code:04X}");
            }
        }
    }
}
