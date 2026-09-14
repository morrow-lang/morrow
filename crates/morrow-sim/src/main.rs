#![forbid(unsafe_code)]
mod cli;
fn main() {
    let arguments = std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "arguments must be valid UTF-8".to_owned())
        })
        .collect::<Result<Vec<_>, _>>();
    if let Err(message) = arguments.and_then(|arguments| cli::run(arguments.into_iter())) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
