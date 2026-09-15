//! Exercise current language features without changing the retained legacy seed contract.
use super::*;

const SOURCES: &[(&str, &str)] = &[
    ("tour", include_str!("../../../examples/language_tour.mr")),
    (
        "custom-json",
        include_str!("../../../crates/morrow/tests/custom_json_native/values.mr"),
    ),
    (
        "comptime",
        include_str!("../../../crates/morrow/tests/comptime_native/values.mr"),
    ),
    (
        "deep-json",
        include_str!("../../../crates/morrow/tests/json_deep_unions/values.mr"),
    ),
    (
        "traits",
        include_str!("../../../crates/morrow/tests/traits/values.mr"),
    ),
    (
        "foreign",
        "foreign \"C\" fn absolute(value: Int) -> Int as \"llabs\"\nfn main(): println(absolute(-42))\n",
    ),
    (
        "actors",
        "fn worker():\n    defer println(\"🌿 cleanup\")\n    for index in 0..3:\n        receive:\n            0 -> println(index)\n            number -> println(number + index)\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    match send(pid, 7):\n        Ok(()) -> ()\n        Err(_) -> ()\n",
    ),
];

pub(super) fn run(path: &Path, seed: u64, invoke: &mut Invoke<'_>) -> Result<usize, String> {
    let mut random = seed ^ 0xD1B5_4A32_D192_ED03;
    let mut cases = 0;
    for (name, original) in SOURCES {
        // The original also goes through check/emit, so source acceptance and
        // formatting invariants cover each complete feature combination.
        for index in 0..33 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let source = if index == 0 {
                (*original).to_owned()
            } else {
                edit(original, random, index % 4)
            };
            fs::write(path, &source).map_err(|error| error.to_string())?;
            if index == 0 {
                require_success("feature check", invoke("check", path)?)?;
                require_success("feature emit", invoke("emit", path)?)?;
            }
            mutation(&source, path, invoke).map_err(|error| format!("language feature={name} seed={seed:#x} index={index}: {error}\nsource={source:?}"))?;
            cases += 1;
        }
    }
    Ok(cases)
}

fn edit(source: &str, random: u64, mode: usize) -> String {
    let boundaries: Vec<_> = source
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(source.len()))
        .collect();
    let selected = random as usize % boundaries.len();
    let start = boundaries[selected];
    let end = boundaries.get(selected + 1).copied().unwrap_or(start);
    let mut output = source.to_owned();
    match mode {
        0 => output.truncate(start),
        1 => output.replace_range(start..end, ""),
        2 => output.insert_str(start, "\n    "),
        _ => output.insert_str(
            start,
            [")", "?", "|", "\"", ":", "🌿"][(random >> 32) as usize % 6],
        ),
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_preserve_utf8_and_have_independent_boundary_oracles() {
        assert_eq!(edit("a🌿z", 1, 0), "a");
        assert_eq!(edit("a🌿z", 1, 1), "az");
        assert_eq!(edit("a🌿z", 1, 2), "a\n    🌿z");
        assert_eq!(edit("a🌿z", 1, 3), "a)🌿z");
        assert_eq!(edit("a🌿z", 3, 1), "a🌿z");
    }
}
