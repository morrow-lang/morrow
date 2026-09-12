//! Shared source cases separate ordinary type validity from Result handling obligations.
use fern_compiler::{check, parse};
#[path = "support/mutual_tree_cases.rs"]
mod corpus;
#[test]
fn complete_mutual_handlers_and_unsafe_paths_keep_distinct_outcomes() {
    for (name, accepted, source) in corpus::cases() {
        let ast = parse::parse(&source).unwrap_or_else(|e| panic!("{name}: {e:?}\n{source}"));
        let result = check::check_library(&ast);
        if accepted {
            result.unwrap_or_else(|e| panic!("{name}: {e:?}\n{source}"));
        } else {
            let error = result.expect_err(name);
            assert!(
                error.message.contains("Result obligation"),
                "{name}: {error:?}"
            );
        }
    }
}
