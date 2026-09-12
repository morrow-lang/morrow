//! Safe terminal editing delegates platform raw-mode handling to pinned Rustyline.
use super::Input;
use rustyline::{
    completion::{Completer, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    history::DefaultHistory,
    validate::Validator,
};
use rustyline::{Config, Context, Editor, Helper};
use std::{io::Read, path::PathBuf};

struct Completion;
impl Helper for Completion {}
impl Highlighter for Completion {}
impl Validator for Completion {}
impl Hinter for Completion {
    type Hint = String;
}
impl Completer for Completion {
    type Candidate = Pair;
    /// Complete the token before the cursor without changing any trailing source.
    fn complete(
        &self,
        line: &str,
        position: usize,
        _: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let Some(prefix) = line.get(..position) else {
            return Ok((position, vec![]));
        };
        let start = prefix
            .rfind(|c: char| !c.is_alphanumeric() && !matches!(c, '_' | '.' | ':'))
            .map_or(0, |at| {
                at + prefix[at..].chars().next().map_or(0, char::len_utf8)
            });
        let word = &prefix[start..];
        let mut words = crate::runtime::names();
        words.extend([
            "print", "println", "fn", "let", "if", "else", "match", "for", "while", "loop",
            "return", "break", "continue", "true", "false", "and", "or", "not", "type", "newtype",
            "pub", "import", "module", "defer", "with", "in", "as", "Ok", "Err", "Some", "None",
            ":help", ":type", ":clear", ":reset", ":quit", ":paste", ":end",
        ]);
        words.sort_unstable();
        words.dedup();
        Ok((
            start,
            words
                .into_iter()
                .filter(|candidate| !word.is_empty() && candidate.starts_with(word))
                .map(|candidate| Pair {
                    display: candidate.into(),
                    replacement: candidate.into(),
                })
                .collect(),
        ))
    }
}

/// Retain at most 1000 history entries; unavailable history never prevents evaluation.
pub(super) fn serve(quiet: bool) -> Result<(), String> {
    let config = Config::builder()
        .max_history_size(1000)
        .map_err(|error| error.to_string())?
        .build();
    let mut editor = Editor::<Completion, DefaultHistory>::with_config(config)
        .map_err(|error| error.to_string())?;
    editor.set_helper(Some(Completion));
    let history = history_path();
    if let Some(path) = &history {
        if let Ok(file) = std::fs::File::open(path) {
            if file.metadata().is_ok_and(|metadata| metadata.is_file()) {
                let mut text = String::new();
                if file
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_string(&mut text)
                    .is_ok()
                {
                    for entry in history_entries(&text) {
                        let _ = editor.add_history_entry(entry);
                    }
                }
            }
        }
    }
    let result = super::serve_lines(std::io::stdout().lock(), !quiet, |ready, _| {
        let prompt = if quiet {
            ""
        } else if ready {
            "fern> "
        } else {
            "...   "
        };
        match editor.readline(prompt) {
            Ok(line) => {
                if line.len() > 1024 * 1024 {
                    return Err("interactive input limit exceeded".into());
                }
                if line.len() <= 4096 && !line.starts_with(':') && !line.trim().is_empty() {
                    let _ = editor.add_history_entry(&line);
                }
                Ok(Input::Line(line))
            }
            Err(ReadlineError::Interrupted) => Ok(Input::Interrupted),
            Err(ReadlineError::Eof) => Ok(Input::End),
            Err(error) => Err(error.to_string()),
        }
    });
    if let Some(path) = history {
        let _ = editor.save_history(&path);
    }
    result
}

/// Preserve the C history location while allowing isolated/custom editor sessions.
fn history_path() -> Option<PathBuf> {
    std::env::var_os("FERN_REPL_HISTORY")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".fern_history")))
        .filter(|path| std::fs::metadata(path).map_or(true, |metadata| metadata.is_file()))
}

/// Bound history bytes, decoded entry size and retained count before exposing recall.
fn history_entries(text: &str) -> Vec<String> {
    if text.len() > 8 * 1024 * 1024 {
        return Vec::new();
    }
    let (escaped, text) = text
        .strip_prefix("#V2\n")
        .map_or((false, text), |text| (true, text));
    let mut entries: Vec<_> = text
        .lines()
        .rev()
        .filter_map(|line| {
            if line.len() > 8192 || line.trim().is_empty() {
                return None;
            }
            let entry = if escaped {
                unescape_history(line)?
            } else {
                line.to_owned()
            };
            (entry.len() <= 4096).then_some(entry)
        })
        .take(1000)
        .collect();
    entries.reverse();
    entries
}

/// Rustyline v2 history escapes backslashes and embedded newlines; C history stays literal.
fn unescape_history(line: &str) -> Option<String> {
    let mut result = String::new();
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next()? {
                'n' => result.push('\n'),
                '\\' => result.push('\\'),
                _ => return None,
            }
        } else {
            result.push(character);
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bounded_history_import_accepts_legacy_and_escaped_rustyline_records() {
        assert_eq!(
            super::history_entries("40 + 2\nString.len(\"a\\n\")\n"),
            vec!["40 + 2", "String.len(\"a\\n\")"]
        );
        assert_eq!(
            super::history_entries("#V2\nString.len(\"a\\\\n\")\n"),
            vec!["String.len(\"a\\n\")"]
        );
        let oversized = format!("40 + 2\n{}\n", "x".repeat(4097));
        assert_eq!(super::history_entries(&oversized), vec!["40 + 2"]);
        let excessive = format!("40 + 2\n{}", "x".repeat(8 * 1024 * 1024));
        assert!(super::history_entries(&excessive).is_empty());
    }
}
