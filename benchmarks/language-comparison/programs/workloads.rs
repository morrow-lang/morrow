use std::env;

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

fn model(steps: i64, seed: i64, mutable: bool) -> (i64, i64) {
    let original: Vec<_> = (0..256).map(|index| Cell { index, value: 0 }).collect();
    let mut values = original.clone();
    for step in 0..steps {
        let target = (seed + step * 17) % 256;
        if mutable {
            values[target as usize].value += 1;
        } else {
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
    match args[1].as_str() {
        "scalar" => println!("{}", scalar(steps, seed)),
        "precision" => println!("{}", 9_007_199_254_740_993_i64 + seed),
        mode => {
            let (sum, old) = model(steps, seed, mode == "mutable");
            println!("{sum}\n{old}");
        }
    }
}
