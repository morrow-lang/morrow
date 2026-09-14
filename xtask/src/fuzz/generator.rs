//! Stable grammar generator. Wrapping arithmetic and RNG draw order are part of the seed contract.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(2_685_821_657_736_338_717)
    }
    fn range(&mut self, limit: u64) -> usize {
        (self.next() % limit) as usize
    }
    fn terminal(&mut self) -> String {
        match self.range(4) {
            0 => self.range(1000).to_string(),
            1 => ["true", "false"][self.range(2)].into(),
            2 => ["x", "y", "value", "n", "count"][self.range(5)].into(),
            _ => ["\"morrow\"", "\"fuzz\"", "\"seed\"", "\"ok\""][self.range(4)].into(),
        }
    }
    fn expressions(&mut self, depth: usize, count: usize) -> String {
        (0..count)
            .map(|_| self.expression(depth - 1))
            .collect::<Vec<_>>()
            .join(", ")
    }
    fn expression(&mut self, depth: usize) -> String {
        if depth == 0 {
            return self.terminal();
        }
        match self.range(7) {
            0 => self.terminal(),
            1 => {
                let op = ["-", "not "][self.range(2)];
                format!("{op}({})", self.expression(depth - 1))
            }
            2 => {
                let op = ["+", "-", "*", "/", "==", "!=", "<", ">", "and", "or"][self.range(10)];
                format!(
                    "({} {op} {})",
                    self.expression(depth - 1),
                    self.expression(depth - 1)
                )
            }
            3 => {
                let count = self.range(3);
                let name = ["add", "compute", "mix", "f", "g"][self.range(5)];
                format!("{name}({})", self.expressions(depth, count))
            }
            4 => {
                let count = self.range(3);
                format!("[{}]", self.expressions(depth, count))
            }
            5 => {
                let count = 2 + self.range(2);
                format!("({})", self.expressions(depth, count))
            }
            _ => format!("({})", self.expression(depth - 1)),
        }
    }
}
/// Generate one independent bounded case. The index selects six layout/program modes.
pub fn generate(seed: u64, index: u32) -> String {
    let mut mixed =
        seed.wrapping_add(0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(u64::from(index.wrapping_add(1))));
    mixed ^= mixed >> 30;
    mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed ^= mixed >> 27;
    mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^= mixed >> 31;
    let mut rng = Rng(if mixed == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        mixed
    });
    rng.next();
    let depth = 2 + rng.range(2);
    match index % 6 {
        0 => format!("fn main() -> Int:\n\tlet x: Int = {}\n\t1\n", rng.expression(depth)),
        1 => {
            let number = 1 + rng.range(100);
            format!("fn choose(n: Int) -> Int:\n\tif n > 0: {}\n\telse: {}\n\nfn main() -> Int:\n\tchoose({number})\n", rng.expression(depth), rng.expression(depth))
        }
        2 => {
            let number = rng.range(5);
            format!("fn classify(n: Int) -> Int:\n\tmatch n: 0 -> {}, _ -> {}\n\nfn main() -> Int:\n\tclassify({number})\n", rng.expression(depth), rng.expression(depth))
        }
        3 => format!("fn combine(x: Int, y: Int):\n\twith a <- Ok(x), b <- Ok(y) do Ok(a + b) else Err(e) -> {}\n\nfn main():\n\tcombine(1, 2)\n", rng.expression(depth)),
        4 => format!("fn id(n: Int) -> Int:\n\tn\n\nfn main() -> Int:\n\tlet value: Int = id({})\n\tvalue\n", rng.expression(depth)),
        _ => "fn layout_demo(n: Int) -> Int:\n\tif n > 10:\n\t\tmatch n:\n\t\t\t11 -> 11\n\t\t\t_ -> n\n\telse:\n\t\twith x <- Ok(n) do x else Err(e) -> 0\n\nfn main() -> Int:\n\tlayout_demo(11)\n".into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_original_512_sources() {
        let expected: Vec<String> =
            serde_json::from_str(include_str!("../../../fuzz/generated-c0ffee.json")).unwrap();
        assert_eq!(expected.len(), 512);
        for (index, source) in expected.iter().enumerate() {
            assert_eq!(&generate(0xC0FFEE, index as u32), source, "case {index}");
        }
    }
}
