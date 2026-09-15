use morrow_compiler::{check, format, lowering, parse};
use std::{fs, path::Path};

#[test]
fn all_native_with_and_iteration_fixtures_preserve_checked_emission() {
    for directory in ["with", "iteration"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(directory);
        let mut files = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "mr"))
            .collect::<Vec<_>>();
        files.sort();
        for file in files {
            let source = fs::read_to_string(&file).unwrap();
            let canonical = format::format(&source)
                .unwrap_or_else(|error| panic!("{}: {error:?}", file.display()));
            assert_eq!(format::format(&canonical).unwrap(), canonical);
            let before =
                lowering::emit(&check::check(&parse::parse(&source).unwrap()).unwrap()).unwrap();
            let after =
                lowering::emit(&check::check(&parse::parse(&canonical).unwrap()).unwrap()).unwrap();
            assert_eq!(before, after, "{}", file.display());
        }
    }
}
