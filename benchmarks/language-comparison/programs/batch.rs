// Repeat the historical workloads.rs scalar and immutable model in one process.
// Runtime arguments: scalar|model|precision, steps, seed, repeats.
use std::{env, hint::black_box};

#[derive(Clone, Copy)]
struct Cell {
    index: i64,
    value: i64,
}

fn scalar(steps: i64, mut value: i64) -> i64 {
    for _ in 0..steps {
        value = value * 48_271 % 2_147_483_647;
    }
    value
}

fn model(steps: i64, seed: i64) -> (i64, i64) {
    let original: Vec<_> = (0..256).map(|index| Cell { index, value: 0 }).collect();
    let mut values = original.clone();
    for step in 0..steps {
        let target = (seed + step * 17) % 256;
        // Keep every old version unmodified; allocate the next whole vector.
        values = values
            .iter()
            .map(|cell| {
                if cell.index == target {
                    Cell {
                        value: cell.value + 1,
                        ..*cell
                    }
                } else {
                    *cell
                }
            })
            .collect();
    }
    (
        values
            .iter()
            .map(|cell| (cell.index + 1) * cell.value)
            .sum(),
        original.iter().map(|cell| cell.value).sum(),
    )
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let steps: i64 = args[2].parse().expect("controlled decimal steps");
    let seed: i64 = args[3].parse().expect("controlled decimal seed");
    let repeats: u64 = args[4].parse().expect("controlled decimal repeats");
    for _ in 0..repeats {
        // Each repeat must perform the work even when inputs are unchanged.
        let (steps, seed) = black_box((steps, seed));
        match args[1].as_str() {
            "scalar" => println!("{}", black_box(scalar(steps, seed))),
            "precision" => println!("{}", black_box(9_007_199_254_740_993_i64 + seed)),
            "model" => {
                let (sum, old) = black_box(model(steps, seed));
                println!("{sum}\n{old}");
            }
            mode => panic!("unsupported controlled mode: {mode}"),
        }
    }
}
