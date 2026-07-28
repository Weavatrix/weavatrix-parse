//! Structural extraction for SQL.
//!
//! SQL declares objects rather than functions and depends on other objects by
//! name rather than by path, so the fact shapes mean something slightly
//! different here: a `CREATE` is a declaration, and every table a statement
//! reads or writes is an import whose specifier is the object name. That is
//! what makes a view resolvable to the file that creates the table it selects
//! from, which is the edge repository intelligence actually wants.
//!
//! Keywords are matched case-insensitively because SQL is written both ways,
//! often in the same file.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one SQL source.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Sql)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        object: None,
    };
    state.run();
    state.facts
}

/// The object keyword to the kind it declares.
const OBJECTS: &[(&str, DeclarationKind)] = &[
    ("table", DeclarationKind::Table),
    ("view", DeclarationKind::View),
    ("function", DeclarationKind::Function),
    ("procedure", DeclarationKind::Procedure),
    ("trigger", DeclarationKind::Procedure),
    ("schema", DeclarationKind::Module),
    ("type", DeclarationKind::TypeAlias),
];

/// Words that qualify a `CREATE` without naming what it creates.
const CREATE_MODIFIERS: &[&str] = &[
    "or",
    "replace",
    "temp",
    "temporary",
    "unique",
    "materialized",
    "global",
    "local",
    "if",
    "not",
    "exists",
];

/// Keywords a referenced object name follows.
const REFERENCES: &[&str] = &["from", "join", "into", "update", "references", "on"];

/// Words that read as an object name but never are one.
const NOT_A_NAME: &[&str] = &[
    "select",
    "lateral",
    "only",
    "delete",
    "conflict",
    "duplicate",
    "set",
    "values",
    "all",
    "distinct",
];

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    /// The object being created, which owns everything until the statement ends.
    object: Option<String>,
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

    fn word(&self, index: usize, keyword: &str) -> bool {
        self.kind(index) == Some(TokenKind::Identifier)
            && self.text(index).eq_ignore_ascii_case(keyword)
    }

    fn any_word(&self, index: usize, keywords: &[&str]) -> bool {
        self.kind(index) == Some(TokenKind::Identifier)
            && keywords
                .iter()
                .any(|keyword| self.text(index).eq_ignore_ascii_case(keyword))
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

    fn step(&mut self, index: usize) -> usize {
        if self.punct(index, ";") {
            self.object = None;
            return index + 1;
        }
        if self.kind(index) != Some(TokenKind::Identifier) {
            return index + 1;
        }
        if (self.word(index, "create") || self.word(index, "alter"))
            && let Some(next) = self.create(index)
        {
            return next;
        }
        if self.any_word(index, REFERENCES)
            && let Some(next) = self.reference(index)
        {
            return next;
        }
        if let Some(next) = self.call(index) {
            return next;
        }
        index + 1
    }

    /// Reads a possibly qualified name: `schema.table`, `"quoted".name`.
    /// Returns the joined name and the index after it.
    fn qualified_name(&self, start: usize) -> Option<(String, usize)> {
        if self.kind(start) != Some(TokenKind::Identifier)
            && self.kind(start) != Some(TokenKind::String)
        {
            return None;
        }
        if self.any_word(start, NOT_A_NAME) {
            return None;
        }
        let mut name = self
            .text(start)
            .trim_matches(['"', '`', '[', ']'])
            .to_owned();
        let mut cursor = start + 1;
        while self.punct(cursor, ".") && self.kind(cursor + 1) == Some(TokenKind::Identifier) {
            name.push('.');
            name.push_str(self.text(cursor + 1).trim_matches(['"', '`', '[', ']']));
            cursor += 2;
        }
        Some((name, cursor))
    }

    /// `CREATE [OR REPLACE] TABLE [IF NOT EXISTS] name`, and the `ALTER TABLE
    /// name` form, which references an existing object rather than declaring one.
    fn create(&mut self, index: usize) -> Option<usize> {
        let altering = self.word(index, "alter");
        let mut cursor = index + 1;
        // `CREATE INDEX name ON table` declares nothing worth a node, but the
        // table it indexes is a real dependency, so it falls through to the
        // reference path via its `ON`.
        while self.any_word(cursor, CREATE_MODIFIERS) {
            cursor += 1;
        }
        let keyword = self.text(cursor);
        let (_, kind) = OBJECTS
            .iter()
            .find(|(word, _)| keyword.eq_ignore_ascii_case(word))?;
        cursor += 1;
        while self.any_word(cursor, CREATE_MODIFIERS) {
            cursor += 1;
        }
        let (name, after) = self.qualified_name(cursor)?;
        if altering {
            self.facts.imports.push(Import {
                specifier: name,
                span: self.span(index, after.saturating_sub(1)),
                type_only: false,
                reexport: false,
                names: Vec::new(),
            });
            return Some(after);
        }
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: *kind,
            span: self.span(index, after.saturating_sub(1)),
            owner: None,
            // Every SQL object is visible to whoever can reach the schema.
            exported: true,
        });
        self.object = Some(name);
        Some(after)
    }

    /// The object names after `FROM`, `JOIN`, `INTO`, `UPDATE`, `REFERENCES`
    /// and `ON`, including comma-separated lists.
    fn reference(&mut self, index: usize) -> Option<usize> {
        let mut cursor = index + 1;
        // A parenthesised subquery is read on its own, and `ON` outside a
        // `CREATE INDEX` introduces a join condition rather than a table.
        if self.punct(cursor, "(") {
            return None;
        }
        if self.word(index, "on") && !self.creating_index(index) {
            return None;
        }
        // Whether the statement reads the object or writes it is the whole
        // point of the edge: a report that reads a table and a job that
        // rewrites it are not the same dependency.
        let writing = self.word(index, "into") || self.word(index, "update");
        let mut recorded = 0_usize;
        while let Some((name, after)) = self.qualified_name(cursor) {
            self.facts.references.push(Reference {
                name: name.clone(),
                kind: if writing {
                    ReferenceKind::Writes
                } else {
                    ReferenceKind::Reads
                },
                receiver: None,
                span: self.span(cursor, after.saturating_sub(1)),
                owner: self.object.clone(),
                string_arguments: Vec::new(),
                name_arguments: Vec::new(),
            });
            self.facts.imports.push(Import {
                specifier: name,
                span: self.span(cursor, after.saturating_sub(1)),
                type_only: false,
                reexport: false,
                names: Vec::new(),
            });
            recorded += 1;
            cursor = after;
            // `FROM a, b` lists several tables; an alias before the comma is
            // skipped because only the first name in an item is the object.
            while self.kind(cursor) == Some(TokenKind::Identifier) && !self.punct(cursor, ",") {
                if self.any_word(cursor, REFERENCES) || self.punct(cursor, ";") {
                    break;
                }
                cursor += 1;
            }
            if !self.punct(cursor, ",") {
                break;
            }
            cursor += 1;
        }
        (recorded > 0).then_some(cursor)
    }

    /// Whether this `ON` belongs to a `CREATE INDEX ... ON table` statement.
    fn creating_index(&self, index: usize) -> bool {
        let start = index.saturating_sub(8);
        (start..index).any(|cursor| {
            self.word(cursor, "index") && (start..cursor).any(|back| self.word(back, "create"))
        })
    }

    fn call(&mut self, index: usize) -> Option<usize> {
        if !self.punct(index + 1, "(") {
            return None;
        }
        let name = self.text(index).to_owned();
        // A type name or a clause keyword can be followed by a parenthesis
        // without being a call.
        if self.any_word(index, NOT_A_NAME)
            || self.any_word(
                index,
                &[
                    "varchar",
                    "char",
                    "decimal",
                    "numeric",
                    "in",
                    "values",
                    "table",
                    "on",
                    "using",
                    "check",
                    "primary",
                    "foreign",
                    "key",
                    "references",
                    "unique",
                    "index",
                ],
            )
        {
            return None;
        }
        let mut arguments = Vec::new();
        let mut scan = index + 2;
        let mut depth = 1_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if depth == 1 && self.kind(scan) == Some(TokenKind::String) {
                arguments.push(self.text(scan).trim_matches('\'').to_owned());
            }
            scan += 1;
        }
        self.facts.references.push(Reference {
            kind: ReferenceKind::Call,
            name,
            receiver: None,
            span: self.span(index, index),
            owner: self.object.clone(),
            string_arguments: arguments,
            name_arguments: Vec::new(),
        });
        Some(index + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::DeclarationKind;

    fn specifiers(source: &str) -> Vec<String> {
        extract(source)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect()
    }

    #[test]
    fn created_objects_carry_their_kind() {
        let source = "CREATE TABLE app.users (id INT);\n\
             create or replace view active_users as select 1;\n\
             CREATE OR REPLACE FUNCTION bump() RETURNS INT AS 'select 1';\n";
        let declared = extract(source)
            .declarations
            .into_iter()
            .map(|item| (item.name, item.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            [
                ("app.users".to_owned(), DeclarationKind::Table),
                ("active_users".to_owned(), DeclarationKind::View),
                ("bump".to_owned(), DeclarationKind::Function),
            ],
            "keywords are matched whichever case they are written in"
        );
    }

    #[test]
    fn a_view_depends_on_every_table_it_reads() {
        let source = "CREATE VIEW report AS\n\
             SELECT o.id\n\
             FROM orders o\n\
             JOIN app.customers c ON c.id = o.customer_id\n\
             LEFT JOIN payments ON payments.order_id = o.id;\n";
        assert_eq!(
            specifiers(source),
            ["orders", "app.customers", "payments"],
            "an ON inside a join is a condition, not another table"
        );
    }

    #[test]
    fn writes_and_alterations_are_dependencies_too() {
        let source = "INSERT INTO events (id) VALUES (1);\n\
             UPDATE accounts SET balance = 0;\n\
             ALTER TABLE accounts ADD COLUMN note TEXT;\n\
             CREATE INDEX idx_events_id ON events (id);\n";
        assert_eq!(
            specifiers(source),
            ["events", "accounts", "accounts", "events"],
            "an ON after CREATE INDEX names the indexed table"
        );
    }

    #[test]
    fn a_comment_or_string_never_declares_a_table() {
        let source = "-- CREATE TABLE ghost (id INT);\n\
             /* CREATE TABLE also_ghost (id INT); */\n\
             INSERT INTO log (message) VALUES ('select * from phantom');\n\
             CREATE TABLE real_one (id INT);\n";
        let facts = extract(source);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["real_one"]
        );
        assert_eq!(
            specifiers(source),
            ["log"],
            "the table named inside the string literal is text"
        );
    }

    #[test]
    fn a_comma_separated_from_list_names_every_table() {
        assert_eq!(
            specifiers("SELECT * FROM users u, orders o, app.items;"),
            ["users", "orders", "app.items"],
            "an alias is not a table"
        );
    }
}
