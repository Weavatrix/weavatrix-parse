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

impl Extractor<'_, '_> {
    fn run(&mut self) {
        let mut index = 0;
        while index < self.tokens.len() {
            index = self.step(index);
        }
    }

    fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let last_index = self.tokens.len().saturating_sub(1);
        let first = &self.tokens[start.min(last_index)];
        let last = &self.tokens[end.min(last_index)];
        Span {
            start: first.start,
            end: last.end,
            line: first.line,
            column: first.column,
            end_line: last.line,
            end_column: last.column,
        }
    }

    /// One shell word: every token written without a space between them, which
    /// is how `./lib/common.sh` and `$HOME/bin` are one argument each.
    fn word(&self, start: usize) -> (String, usize) {
        let mut text = String::new();
        let mut cursor = start;
        if start >= self.tokens.len() {
            return (text, start);
        }
        while cursor < self.tokens.len() {
            let token = &self.tokens[cursor];
            if cursor > start && self.tokens[cursor - 1].end != token.start {
                break;
            }
            if token.line != self.tokens[start].line {
                break;
            }
            let raw = token.text(self.source);
            if token.kind == TokenKind::String {
                text.push_str(raw.trim_matches(['"', '\'']));
            } else {
                text.push_str(raw);
            }
            cursor += 1;
        }
        (text, cursor)
    }

    /// Every word of the statement starting at `index`, up to its end.
    fn words(&self, start: usize) -> Vec<String> {
        let mut found = Vec::new();
        let Some(line) = self.tokens.get(start).map(|token| token.line) else {
            return found;
        };
        let mut cursor = start;
        while cursor < self.tokens.len() && self.tokens[cursor].line == line {
            // A pipe or a separator ends this command's arguments.
            if self.punct(cursor, ";") || self.punct(cursor, "|") || self.punct(cursor, "&") {
                break;
            }
            let (word, next) = self.word(cursor);
            if next == cursor {
                break;
            }
            if !word.is_empty() {
                found.push(word);
            }
            cursor = next;
        }
        found
    }

    fn step(&mut self, index: usize) -> usize {
        if self.punct(index, "{") {
            self.depth += 1;
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            if self
                .function
                .as_ref()
                .is_some_and(|(_, depth)| self.depth < *depth)
            {
                self.function = None;
            }
            return index + 1;
        }
        // `. lib.sh` and `./deploy.sh` are commands whose first character is
        // punctuation, so a word may begin with one.
        let opens_a_word = self.kind(index) == Some(TokenKind::Identifier)
            || self.punct(index, ".")
            || self.punct(index, "/");
        if !opens_a_word || !self.starts_a_statement(index) {
            return index + 1;
        }
        if let Some(next) = self.definition(index) {
            return next;
        }
        self.command(index)
    }

    /// Whether this word is the first of a command rather than an argument.
    fn starts_a_statement(&self, index: usize) -> bool {
        if index == 0 {
            return true;
        }
        let previous = &self.tokens[index - 1];
        if previous.line != self.tokens[index].line {
            return true;
        }
        // A word joined to the one before it is part of it, not a new command.
        if previous.end == self.tokens[index].start {
            return false;
        }
        matches!(previous.text(self.source), ";" | "|" | "&" | "(" | "{")
            || KEYWORDS.contains(&previous.text(self.source))
    }

    /// `function deploy {`, `deploy() {`.
    fn definition(&mut self, index: usize) -> Option<usize> {
        let (name_index, after) = if self.text(index) == "function" {
            (index + 1, index + 2)
        } else if self.punct(index + 1, "(") && self.punct(index + 2, ")") {
            (index, index + 3)
        } else {
            return None;
        };
        if self.kind(name_index) != Some(TokenKind::Identifier) {
            return None;
        }
        // `function name()` writes both forms; either way a brace follows.
        let mut cursor = after;
        while cursor < self.tokens.len() && !self.punct(cursor, "{") {
            if self.tokens[cursor].line != self.tokens[index].line {
                return None;
            }
            cursor += 1;
        }
        let name = self.text(name_index).to_owned();
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Function,
            span: self.span(index, name_index),
            owner: None,
            // A shell function is callable by anything that sources the file.
            exported: true,
        });
        self.function = Some((name, self.depth + 1));
        Some(cursor)
    }

    /// A command, which may pull in another script or address a service.
    fn command(&mut self, index: usize) -> usize {
        let (name, after) = self.word(index);
        if name.is_empty() {
            return index + 1;
        }
        let arguments = self.words(after);
        let span = self.span(index, index);

        if RUNNERS.contains(&name.as_str())
            && let Some(script) = arguments.iter().find(|word| !word.starts_with('-'))
        {
            self.facts.imports.push(Import {
                specifier: script.clone(),
                span,
                type_only: false,
                reexport: false,
                names: Vec::new(),
                bindings: Vec::new(),
            });
            return after;
        }
        // Running a sibling script directly is the same dependency.
        let extension = name
            .rsplit_once('.')
            .map(|(_, tail)| tail.to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("sh" | "bash" | "zsh")) {
            self.facts.imports.push(Import {
                specifier: name,
                span,
                type_only: false,
                reexport: false,
                names: Vec::new(),
                bindings: Vec::new(),
            });
            return after;
        }

        let addresses = if CLIENTS.contains(&name.as_str()) {
            endpoints(&arguments)
        } else {
            Vec::new()
        };
        self.facts.references.push(Reference {
            name,
            kind: ReferenceKind::Call,
            receiver: None,
            span,
            owner: self.function.as_ref().map(|(name, _)| name.clone()),
            string_arguments: addresses,
            name_arguments: Vec::new(),
        });
        after
    }
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

#[cfg(test)]
mod tests {
    use super::extract;

    #[test]
    fn a_script_depends_on_what_it_sources_and_what_it_runs() {
        let source = "#!/usr/bin/env bash\n\
             source ./lib/common.sh\n\
             . \"${DIR}/env.sh\"\n\
             bash scripts/migrate.sh --yes\n\
             ./scripts/deploy.sh production\n\
             echo \"source ./ghost.sh\"\n";
        assert_eq!(
            extract(source)
                .imports
                .into_iter()
                .map(|import| import.specifier)
                .collect::<Vec<_>>(),
            [
                "./lib/common.sh",
                "${DIR}/env.sh",
                "scripts/migrate.sh",
                "./scripts/deploy.sh",
            ],
            "a path inside a string argument is text, not a dependency"
        );
    }

    #[test]
    fn a_client_command_records_the_endpoint_it_addresses() {
        let source = "curl -sf http://localhost:8080/api/v1/health\n\
             curl -X POST \"https://api.example.com/v2/jobs\" -d @payload.json\n\
             wget https://cdn.example.com/artifact.tgz\n\
             echo https://not-a-request.example.com\n";
        let addressed = extract(source)
            .references
            .into_iter()
            .filter(|reference| !reference.string_arguments.is_empty())
            .map(|reference| (reference.name, reference.string_arguments))
            .collect::<Vec<_>>();
        assert_eq!(
            addressed,
            [
                (
                    "curl".to_owned(),
                    vec!["http://localhost:8080/api/v1/health".to_owned()]
                ),
                (
                    "curl".to_owned(),
                    vec![
                        "POST".to_owned(),
                        "https://api.example.com/v2/jobs".to_owned()
                    ]
                ),
                (
                    "wget".to_owned(),
                    vec!["https://cdn.example.com/artifact.tgz".to_owned()]
                ),
            ],
            "echo is not a client, so its argument is not an endpoint"
        );
    }

    #[test]
    fn functions_are_declared_and_own_the_commands_inside_them() {
        let source = "deploy() {\n\
             \x20 curl -sf http://svc/ready\n\
             }\n\
             \n\
             function rollback {\n\
             \x20 kubectl rollout undo\n\
             }\n\
             \n\
             deploy\n";
        let facts = extract(source);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["deploy", "rollback"],
            "both spellings of a definition count"
        );
        assert!(
            facts
                .references
                .iter()
                .any(|reference| reference.name == "curl"
                    && reference.owner.as_deref() == Some("deploy")),
            "a command belongs to the function it is written in"
        );
        assert!(
            facts
                .references
                .iter()
                .any(|reference| reference.name == "kubectl"
                    && reference.owner.as_deref() == Some("rollback")),
            "and the next function owns its own"
        );
    }

    #[test]
    fn a_comment_is_not_a_command_and_a_hash_in_a_string_is_not_a_comment() {
        let source = "# curl http://ghost/api\n\
             echo \"a # inside a string\" && curl http://real/api\n";
        let names = extract(source)
            .references
            .into_iter()
            .map(|reference| reference.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"curl".to_owned()), "got {names:?}");
        assert_eq!(
            names.iter().filter(|name| *name == "curl").count(),
            1,
            "only the command after the string, got {names:?}"
        );
    }

    #[test]
    fn an_argument_is_not_read_as_a_command_of_its_own() {
        let source = "docker run --rm -v /tmp:/tmp alpine sh -c 'echo hi'\n";
        let names = extract(source)
            .references
            .into_iter()
            .map(|reference| reference.name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["docker"],
            "everything after the command word is an argument, got {names:?}"
        );
    }
}
