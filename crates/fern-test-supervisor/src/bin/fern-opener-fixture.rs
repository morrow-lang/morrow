//! Immutable Rust launcher fixture for literal-path, failure and timeout tests.
use std::{env, fs, os::unix::ffi::OsStrExt, path::Path, process::ExitCode, time::Duration};

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args_os().collect();
    let executable = Path::new(&arguments[0]);
    if executable
        .file_name()
        .is_some_and(|name| name == "opener-timeout")
    {
        std::thread::sleep(Duration::from_secs(60));
        return ExitCode::SUCCESS;
    }
    let exit = executable.parent().unwrap().join("exit-code");
    if let Ok(code) = fs::read_to_string(exit) {
        return ExitCode::from(code.parse::<u8>().unwrap());
    }
    if let (Some(calls), Some(seen)) = (env::var_os("CALLS"), env::var_os("SEEN")) {
        fs::copy(&arguments[1], seen).unwrap();
        let mut output = Vec::new();
        for argument in &arguments[1..] {
            output.extend_from_slice(argument.as_bytes());
            output.push(0);
        }
        fs::write(calls, output).unwrap();
    } else {
        fs::write(executable.with_extension("result"), arguments[1].as_bytes()).unwrap();
    }
    ExitCode::SUCCESS
}
