use super::extract;
use crate::facts::DeclarationKind;
use crate::syntax::Language;

fn declared(source: &str, language: Language) -> Vec<String> {
    extract(source, language)
        .declarations
        .into_iter()
        .map(|item| item.name)
        .collect()
}

#[test]
fn class_and_id_selectors_are_declarations() {
    let source = ".panel { color: red; }\n\
         #header, .nav .item { margin: 0; }\n\
         a:hover { color: blue; }\n";
    assert_eq!(
        declared(source, Language::Css),
        [".panel", "#header", ".nav", ".item"],
        "a pseudo-class on a bare element declares no selector"
    );
}

#[test]
fn nested_scss_selectors_are_resolved_to_the_names_they_produce() {
    // The JS engine's own note admits this case is under-captured there,
    // because it has no SCSS grammar and reads CSS as flat rules.
    let source = ".card {\n\
         \x20 color: red;\n\
         \x20 &__title { font-weight: bold; }\n\
         \x20 &--wide { width: 100%; }\n\
         \x20 .inner { padding: 0; }\n\
         }\n";
    let names = declared(source, Language::Scss);
    assert!(names.contains(&".card".to_owned()), "got {names:?}");
    assert!(
        names.contains(&".card__title".to_owned()),
        "an ampersand joins the child onto its parent, got {names:?}"
    );
    assert!(names.contains(&".card--wide".to_owned()), "got {names:?}");
    assert!(names.contains(&".inner".to_owned()), "got {names:?}");
}

#[test]
fn stylesheets_name_the_stylesheets_they_pull_in() {
    let source = "@import \"./base.css\";\n\
         @use \"sass:math\";\n\
         @forward \"./theme\";\n\
         .x { background: url(\"./bg.png\"); }\n";
    let imports = extract(source, Language::Scss)
        .imports
        .into_iter()
        .map(|import| (import.specifier, import.reexport))
        .collect::<Vec<_>>();
    assert_eq!(
        imports,
        [
            ("./base.css".to_owned(), false),
            ("sass:math".to_owned(), false),
            ("./theme".to_owned(), true),
        ],
        "a forward re-exports, and a url() inside a property is not an import"
    );
}

#[test]
fn a_comment_declares_no_selector_and_a_decimal_is_not_a_class() {
    let source = "/* .ghost { } */\n\
         .real { margin: .5em; padding: 0 .25rem; }\n";
    assert_eq!(declared(source, Language::Css), [".real"]);
}

#[test]
fn a_double_slash_is_a_comment_in_scss_and_not_in_css() {
    let scss = "// .ghost { }\n.real { }\n";
    assert_eq!(declared(scss, Language::Scss), [".real"]);
    // In plain CSS `//` is not a comment, so the selector after it on the
    // same line is still read rather than silently dropped.
    assert_eq!(
        extract("// x\n.real { }\n", Language::Css)
            .declarations
            .len(),
        1
    );
}

#[test]
fn selectors_carry_the_kind_the_graph_stores_them_under() {
    let facts = extract(".only { }\n", Language::Css);
    assert_eq!(facts.declarations[0].kind, DeclarationKind::Selector);
}
