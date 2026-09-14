//! Independent mathematical oracles, not translations of measured loops.
pub fn scalar(steps: u64, seed: u64) -> u64 {
    let (mut power, mut exponent, mut factor) = (1_u64, steps, 48_271_u64);
    while exponent != 0 {
        if exponent & 1 != 0 {
            power = power * factor % 2_147_483_647;
        }
        factor = factor * factor % 2_147_483_647;
        exponent >>= 1;
    }
    seed * power % 2_147_483_647
}

pub fn model(steps: u64, seed: u64) -> u64 {
    (0..256_u64)
        .map(|index| {
            // 241 is the inverse of 17 modulo 256. Count visits directly.
            let first = ((index + 256 - seed % 256) * 241) % 256;
            let count = if steps <= first {
                0
            } else {
                1 + (steps - 1 - first) / 256
            };
            (index + 1) * count
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independently_known_values() {
        assert_eq!(scalar(0, 1), 1);
        assert_eq!(scalar(1, 1), 48_271);
        assert_eq!(scalar(2, 1), 182_605_794);
        assert_eq!(scalar(10_000, 1), 399_268_537);
        assert_eq!(model(0, 7), 0);
        assert_eq!(model(1, 7), 8);
        assert_eq!(model(2, 7), 33);
        assert_eq!(model(256, 7), 32_896);
        assert_eq!(model(512, 1), 65_792);
    }
}
