//! Structural extraction for the brace-scoped languages.
//!
//! Rust, Go, Java, C#, C, C++ and Solidity differ in which keyword introduces
//! a declaration and how a module is named, and agree on everything else:
//! braces open bodies, a name followed by a parameter list is callable, and a
//! call is an identifier followed by `(`. Those differences are tables, so one
//! walk serves all seven instead of seven near-identical scanners - and adding
//! the next such language costs a table, not a scanner.

use crate::facts::{
    Declaration, DeclarationKind, Facts, Import, ImportBinding, Reference, ReferenceKind, Span,
};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one brace-scoped source file.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        language,
        rules: Rules::of(language),
        facts: Facts::default(),
        scopes: Vec::new(),
        depth: 0,
    };
    state.run();
    state.facts
}

/// Keywords one language uses, as data.
struct Rules {
    /// Keyword to the kind it declares.
    declarations: &'static [(&'static str, DeclarationKind)],
    /// Keywords that introduce a module dependency.
    imports: &'static [&'static str],
    /// Modifiers to step over before the declaring keyword.
    modifiers: &'static [&'static str],
    /// Whether a bare `name(` at type-body depth declares a method.
    braced_members: bool,
    /// Whether `const (...)` and `var (...)` contain one declaration spec per
    /// top-level line. This is Go syntax, not a generic braced-language rule.
    grouped_declarations: bool,
    /// Whether a function is declared by a return type rather than a keyword,
    /// as C and C++ do: `int add(int a, int b) { }`.
    typed_functions: bool,
    /// Whether a declaration is public by keyword rather than by convention.
    exported_keyword: Option<&'static str>,
    /// Keywords that open a named scope without declaring anything: Rust's
    /// `impl Type` and Swift's `extension Type` say what the members belong
    /// to, and declare no new name.
    scope_keywords: &'static [&'static str],
}

impl Rules {
    const fn of(language: Language) -> Self {
        match language {
            Language::Rust => Self::rust(),
            Language::Swift => Self::swift(),
            Language::Go => Self::go(),
            Language::Java | Language::CSharp => Self::managed(),
            Language::Solidity => Self::solidity(),
            _ => Self::c_family(),
        }
    }

    const fn rust() -> Self {
        Self {
            declarations: &[
                ("fn", DeclarationKind::Function),
                ("struct", DeclarationKind::Struct),
                ("enum", DeclarationKind::Enum),
                ("trait", DeclarationKind::Trait),
                ("type", DeclarationKind::TypeAlias),
                ("const", DeclarationKind::Constant),
                ("static", DeclarationKind::Constant),
                ("mod", DeclarationKind::Module),
            ],
            imports: &["use", "mod"],
            modifiers: &["pub", "async", "unsafe", "extern", "default"],
            braced_members: false,
            grouped_declarations: false,
            typed_functions: false,
            exported_keyword: Some("pub"),
            scope_keywords: &["impl"],
        }
    }

    const fn swift() -> Self {
        Self {
            declarations: &[
                ("func", DeclarationKind::Function),
                ("class", DeclarationKind::Class),
                ("struct", DeclarationKind::Struct),
                ("actor", DeclarationKind::Class),
                ("enum", DeclarationKind::Enum),
                ("protocol", DeclarationKind::Interface),
                ("typealias", DeclarationKind::TypeAlias),
                ("associatedtype", DeclarationKind::TypeAlias),
                ("let", DeclarationKind::Constant),
                ("var", DeclarationKind::Variable),
                ("init", DeclarationKind::Method),
                ("subscript", DeclarationKind::Method),
            ],
            imports: &["import"],
            modifiers: &[
                "public",
                "private",
                "internal",
                "fileprivate",
                "open",
                "static",
                "final",
                "override",
                "mutating",
                "nonmutating",
                "lazy",
                "weak",
                "unowned",
                "required",
                "convenience",
                "indirect",
                "dynamic",
                "optional",
                "async",
                "throws",
            ],
            braced_members: false,
            grouped_declarations: false,
            typed_functions: false,
            // `open` is wider than `public`, but both leave the module.
            exported_keyword: Some("public"),
            scope_keywords: &["extension"],
        }
    }

    const fn go() -> Self {
        Self {
            declarations: &[
                ("func", DeclarationKind::Function),
                ("type", DeclarationKind::Struct),
                ("const", DeclarationKind::Constant),
                ("var", DeclarationKind::Variable),
            ],
            imports: &["import"],
            modifiers: &[],
            braced_members: false,
            grouped_declarations: true,
            typed_functions: false,
            exported_keyword: None,
            scope_keywords: &[],
        }
    }

    const fn managed() -> Self {
        Self {
            declarations: &[
                ("class", DeclarationKind::Class),
                ("interface", DeclarationKind::Interface),
                ("enum", DeclarationKind::Enum),
                ("record", DeclarationKind::Struct),
                ("struct", DeclarationKind::Struct),
            ],
            imports: &["import", "using"],
            modifiers: &[
                "public",
                "private",
                "protected",
                "static",
                "final",
                "abstract",
                "sealed",
                "internal",
                "override",
                "async",
                "virtual",
                "readonly",
            ],
            braced_members: true,
            grouped_declarations: false,
            typed_functions: false,
            exported_keyword: Some("public"),
            scope_keywords: &[],
        }
    }

    const fn solidity() -> Self {
        Self {
            declarations: &[
                ("contract", DeclarationKind::Class),
                ("library", DeclarationKind::Class),
                ("interface", DeclarationKind::Interface),
                ("struct", DeclarationKind::Struct),
                ("enum", DeclarationKind::Enum),
                ("function", DeclarationKind::Function),
                ("constructor", DeclarationKind::Method),
                ("modifier", DeclarationKind::Function),
                ("event", DeclarationKind::Field),
                ("error", DeclarationKind::Struct),
            ],
            imports: &["import"],
            modifiers: &[
                "abstract", "virtual", "override", "public", "private", "internal", "external",
                "pure", "view", "payable",
            ],
            braced_members: false,
            grouped_declarations: false,
            typed_functions: false,
            // Anything not internal or private is reachable from another contract.
            exported_keyword: Some("public"),
            scope_keywords: &[],
        }
    }

    const fn c_family() -> Self {
        Self {
            declarations: &[
                ("struct", DeclarationKind::Struct),
                ("class", DeclarationKind::Class),
                ("enum", DeclarationKind::Enum),
                ("namespace", DeclarationKind::Module),
            ],
            imports: &["#include", "include"],
            modifiers: &["static", "inline", "extern", "const", "virtual"],
            braced_members: true,
            grouped_declarations: false,
            typed_functions: true,
            exported_keyword: None,
            scope_keywords: &[],
        }
    }
}

struct Scope {
    name: String,
    depth: Option<i32>,
    declaration: Option<usize>,
    type_body: bool,
    test_only: bool,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    language: Language,
    rules: Rules,
    facts: Facts,
    scopes: Vec<Scope>,
    depth: i32,
}

mod calls;
mod declarations;
mod fields;
mod imports;
mod scopes;
mod traversal;

#[cfg(test)]
mod tests;
