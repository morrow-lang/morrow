//! Global human-output controls leave artifact and forwarded OS arguments literal.
use std::{ffi::OsString, io::IsTerminal};
#[derive(Clone, Copy, Default)]
enum Color {
    #[default]
    Auto,
    Always,
    Never,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Controls {
    pub(super) quiet: bool,
    verbose: bool,
    color: Color,
}
impl Controls {
    /// Remove recognized global controls while preserving option operands and run's tail.
    pub(super) fn arguments(&mut self, arguments: Vec<OsString>) -> Result<Vec<OsString>, String> {
        if arguments.len() > 4096
            || arguments
                .iter()
                .map(|a| a.as_encoded_bytes().len().saturating_add(1))
                .try_fold(0usize, usize::checked_add)
                .unwrap_or(usize::MAX)
                > 1024 * 1024
        {
            return Err("arguments exceed 4096 words or 1 MiB".into());
        }
        let mut output = Vec::with_capacity(arguments.len());
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--") => {
                    output.push(argument);
                    output.extend(arguments);
                    break;
                }
                Some("-o" | "--output" | "--timeout" | "--backend") => {
                    output.push(argument);
                    if let Some(value) = arguments.next() {
                        output.push(value);
                    }
                }
                Some("--quiet") => self.quiet = true,
                Some("--verbose") => self.verbose = true,
                Some("--color=auto") => self.color = Color::Auto,
                Some("--color=always") => self.color = Color::Always,
                Some("--color=never") => self.color = Color::Never,
                Some(value) if value == "--color" || value.starts_with("--color=") => {
                    return Err("color must be --color=auto|always|never".into());
                }
                Some("-v") if output.is_empty() => output.push("--version".into()),
                _ => output.push(argument),
            }
        }
        Ok(output)
    }
    /// Auto follows the actual destination; explicit always overrides ambient NO_COLOR.
    fn colored(&self, terminal: bool) -> bool {
        match self.color {
            Color::Always => true,
            Color::Never => false,
            Color::Auto => terminal && std::env::var_os("NO_COLOR").is_none(),
        }
    }
    /// Successful compiler status is informational; generated data never passes here.
    pub(super) fn information(&self, message: &str) {
        if !self.quiet {
            if self.colored(std::io::stdout().is_terminal()) {
                println!("\x1b[32m{message}\x1b[0m");
            } else {
                println!("{message}");
            }
        }
    }
    /// Failure diagnostics remain visible in quiet mode and never alter protocol stdout.
    pub(super) fn error(&self, message: &str) {
        if self.colored(std::io::stderr().is_terminal()) {
            eprintln!("\x1b[31m{message}\x1b[0m");
        } else {
            eprintln!("{message}");
        }
    }
    /// Verbose reveals the recognized action, never arbitrary operands or forwarded data.
    pub(super) fn announce(&self, arguments: &[OsString]) {
        if let Some(action) = arguments.first().and_then(|a| a.to_str())
            && self.verbose
            && [
                "check", "emit", "build", "run", "fmt", "doc", "test", "repl", "lsp", "lex",
                "parse",
            ]
            .contains(&action)
        {
            eprintln!("verbose: command={action}");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flags_preserve_output_timeout_operands_and_run_tail() {
        let mut flags = Controls::default();
        let args = ["run", "source.mr", "--", "--quiet", "--color=invalid", "-v"]
            .map(OsString::from)
            .to_vec();
        assert_eq!(flags.arguments(args.clone()).unwrap(), args);
        assert!(!flags.quiet);
        for flag in ["-o", "--output", "--timeout", "--backend"] {
            let args = ["doc", flag, "--quiet"].map(OsString::from).to_vec();
            assert_eq!(flags.arguments(args.clone()).unwrap(), args);
            assert!(!flags.quiet);
        }
    }
    #[test]
    fn oversized_argv_rejects_before_subcommand_dispatch() {
        let mut flags = Controls::default();
        assert!(flags.arguments(vec![OsString::from("x"); 4097]).is_err());
        assert!(
            flags
                .arguments(vec![OsString::from("x".repeat(1024 * 1024))])
                .is_err()
        );
    }
    #[test]
    fn explicit_color_overrides_terminal_detection_and_last_setting_wins() {
        let mut flags = Controls::default();
        assert!(!flags.colored(false));
        flags.arguments(vec!["--color=always".into()]).unwrap();
        assert!(flags.colored(false));
        flags.arguments(vec!["--color=never".into()]).unwrap();
        assert!(!flags.colored(true));
        flags
            .arguments(vec!["--color=always".into(), "--color=never".into()])
            .unwrap();
        assert!(!flags.colored(true));
    }
    #[test]
    fn exact_argument_limits_and_empty_forwarded_words_remain_valid() {
        let mut flags = Controls::default();
        assert_eq!(
            flags.arguments(vec![OsString::new(); 4096]).unwrap().len(),
            4096
        );
        assert!(
            flags
                .arguments(vec!["x".repeat(1024 * 1024 - 1).into()])
                .is_ok()
        );
        let args = ["--verbose", "--quiet", "-v"].map(OsString::from).to_vec();
        assert_eq!(
            flags.arguments(args).unwrap(),
            vec![OsString::from("--version")]
        );
        assert!(flags.quiet && flags.verbose);
    }
    #[cfg(unix)]
    #[test]
    fn non_utf8_operands_are_preserved() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![255, 254]);
        let args = vec!["emit".into(), "source.mr".into(), "-o".into(), value];
        assert_eq!(Controls::default().arguments(args.clone()).unwrap(), args);
    }
}
