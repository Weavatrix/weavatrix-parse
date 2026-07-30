use super::*;

#[test]
fn a_default_import_alongside_named_ones_is_still_an_import() {
    use crate::syntax::Language;

    let source = "import './sideEffect.js';\n\
         import express from 'express';\n\
         import logger, { logRequest, logAction } from './logger.js';\n\
         import * as tty from 'node:tty';\n\
         import type { Config } from './config';\n\
         export { helper } from './helper.js';\n\
         const meta = import.meta.url;\n";
    let facts = super::extract(source, Language::TypeScript);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        [
            "./sideEffect.js",
            "express",
            "./logger.js",
            "node:tty",
            "./config",
            "./helper.js",
        ],
        "a name before the brace must not end the clause, and import.meta \
         is not a module statement"
    );
}

/// React files are the ones where the lexer's assumptions are most
/// fragile: `<` and `>` surround markup rather than comparing, and a
/// slash inside JSX text must stay a division rather than opening a
/// regular expression that would swallow the rest of the file.
#[test]
fn jsx_does_not_derail_the_token_stream() {
    use crate::syntax::Language;
    use crate::token::{TokenKind, tokenize};

    let source = "import React from 'react';\n\
         import { Button } from './Button';\n\
         export function Panel({ items, onPick }) {\n\
         \x20 const ratio = items.length / 2;\n\
         \x20 return (\n\
         \x20   <div className=\"panel\" data-count={items.length}>\n\
         \x20     {items.map((item) => (\n\
         \x20       <Button key={item.id} onClick={() => onPick(item)}>\n\
         \x20         {item.label} / {ratio}\n\
         \x20       </Button>\n\
         \x20     ))}\n\
         \x20   </div>\n\
         \x20 );\n\
         }\n\
         export const Footer = () => <footer>done</footer>;\n";
    let tokens = tokenize(source, Language::TypeScript);
    assert_eq!(
        tokens
            .iter()
            .map(|token| token.text(source))
            .collect::<String>(),
        source,
        "the stream must still reproduce the file"
    );
    assert!(
        !tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Regex | TokenKind::Unterminated)),
        "no division in JSX may be read as a regular expression"
    );
    let facts = super::extract(source, Language::TypeScript);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        ["react", "./Button"]
    );
    for name in ["Panel", "Footer"] {
        assert!(
            facts.declarations.iter().any(|item| item.name == name),
            "{name} must survive the markup, got {:?}",
            facts
                .declarations
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>()
        );
    }
}
#[test]
fn reads_every_module_form_including_multi_line() {
    let source = "import defaultExport from './a';\n\
         import {\n  first,\n  second,\n} from './b';\n\
         import type { Shape } from './c';\n\
         import { type Only } from './d';\n\
         import * as everything from './e';\n\
         import './f';\n\
         const legacy = require('./g');\n\
         const lazy = await import('./h');\n\
         export { thing } from './i';\n\
         export * from './j';\n";
    assert_eq!(
        specifiers(source),
        [
            ("./a".to_owned(), false, false),
            ("./b".to_owned(), false, false),
            ("./c".to_owned(), true, false),
            ("./d".to_owned(), true, false),
            ("./e".to_owned(), false, false),
            ("./f".to_owned(), false, false),
            ("./g".to_owned(), false, false),
            ("./h".to_owned(), false, false),
            ("./i".to_owned(), false, true),
            ("./j".to_owned(), false, true),
        ],
        "a multi-line import is one fact, and type-only is distinguished"
    );
}

#[test]
fn a_comment_or_string_never_becomes_a_fact() {
    let source = "// import { fake } from './nope';\n\
         const text = \"import { alsoFake } from './nope2'\";\n\
         /* app.get('/commented-route', handler); */\n\
         import { real } from './yes';\n\
         app.get('/real-route', handler);\n";
    assert_eq!(specifiers(source), [("./yes".to_owned(), false, false)]);
    let facts = extract(source, Language::TypeScript);
    let routes = facts
        .references
        .iter()
        .flat_map(|call| call.string_arguments.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        routes,
        ["/real-route"],
        "the commented route is not a call argument"
    );
}
