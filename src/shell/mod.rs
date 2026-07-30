//! Structural extraction for shell scripts.
//!
//! Shell is where a repository keeps the things nothing else records. A CI job
//! or a deploy script sources its helpers, invokes its siblings, and calls
//! services by their address - so a script is often the only place an endpoint
//! is written down at all. No other extractor here can see any of that, and
//! most tools do not read shell at all.
//!
//! Words are rebuilt from the token stream by byte adjacency rather than by
//! splitting the line, because that is what keeps a `#` inside a string from
//! ending the line and a quoted URL in one piece.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one shell script.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Bash)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        function: None,
        depth: 0,
    };
    state.run();
    state.facts
}

/// Commands that address a service, and so name an endpoint.
const CLIENTS: &[&str] = &[
    "curl", "wget", "http", "https", "xh", "httpie", "nc", "ncat", "grpcurl", "ab", "hey", "siege",
    "wrk",
];

/// Commands whose first argument is another script to run.
const RUNNERS: &[&str] = &["source", ".", "bash", "sh", "zsh", "ksh"];

/// Words that begin a statement, so the word after one is a command.
const KEYWORDS: &[&str] = &[
    "then", "do", "else", "elif", "fi", "done", "if", "while", "until", "for", "case", "esac",
    "in", "function", "return", "local", "export", "declare", "readonly", "eval", "exec", "time",
];

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    /// The function whose body the walk is inside.
    function: Option<(String, i32)>,
    depth: i32,
}

/// The addresses and method a client command was given.
///
/// A URL in a shell script is usually written unquoted, so it arrives as one
/// word rather than as a string literal - which is why arguments are rebuilt
/// before they are read.
fn endpoints(arguments: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut expecting_method = false;
    for argument in arguments {
        if expecting_method {
            found.push(argument.clone());
            expecting_method = false;
            continue;
        }
        if argument == "-X" || argument == "--request" {
            expecting_method = true;
            continue;
        }
        if argument.contains("://") || argument.starts_with("localhost:") {
            found.push(argument.clone());
        }
    }
    found
}

mod extractor;

#[cfg(test)]
mod tests;
