//! A one-file deployment still carries the notices for the code it distributes.
#[test]
fn licenses_are_available_without_assets_credentials_or_a_listening_socket() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fern-web"))
        .arg("--licenses")
        .env_remove("FERN_WEB_ACCESS_KEY")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(include_str!("../../../LICENSE")));
    assert!(text.contains("# Third-party notices"));
    assert!(text.contains("Permission is hereby granted"));
    assert!(output.stderr.is_empty());
}
