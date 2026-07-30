use super::*;

#[test]
fn rust_module_declarations_are_dependencies_but_inline_modules_are_not() {
    let source = "pub mod engine;\nmod helper;\nuse crate::engine::Driver;\n\
         mod inline { pub fn nested() {} }\npub fn run() { Driver::start(); }\n";
    let specifiers = extract(source, Language::Rust)
        .imports
        .into_iter()
        .map(|import| import.specifier)
        .collect::<Vec<_>>();
    assert_eq!(
        specifiers,
        ["self::engine", "self::helper", "crate::engine::Driver"],
        "a mod with a body defines the module here rather than including a file"
    );
}

#[test]
fn rust_test_attributes_classify_the_declaration_and_nested_scope() {
    let source = r#"
        fn production() {}
        #[cfg(not(test))]
        fn production_without_tests() {}
        #[cfg(any(test, feature = "test-support"))]
        #[allow(dead_code)]
        mod tests {
            #[test]
            fn embedded_test() {}
            fn helper_for_test() {}
        }
        #[tokio::test]
        async fn async_test() {}
        #[cfg(not(not(test)))]
        fn double_negated_test() {}
    "#;
    let facts = extract(source, Language::Rust);
    let test_only = |name: &str| {
        facts
            .declarations
            .iter()
            .find(|declaration| declaration.name == name)
            .is_some_and(|declaration| facts.declaration_is_test_only(declaration.span))
    };
    for production in ["production", "production_without_tests"] {
        assert!(
            !test_only(production),
            "{production} is available to production"
        );
    }
    for test in [
        "tests",
        "embedded_test",
        "helper_for_test",
        "async_test",
        "double_negated_test",
    ] {
        assert!(test_only(test), "{test} is test-only syntax");
    }
}

#[test]
fn a_character_literal_holding_a_quote_does_not_shift_the_rest_of_the_file() {
    // A lifetime and a character literal open the same way, so this used
    // to leave `"` opening a string that ran on for hundreds of lines and
    // swallowed every declaration after it.
    let source = "fn classify<'a>(head: &'a str) -> bool {\n\
         \x20   head.contains(['.', '\"', '+']) || head.starts_with('@')\n\
         }\n\
         mod tests {\n\
         \x20   use super::classify;\n\
         }\n";
    let facts = extract(source, Language::Rust);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        ["super::classify"],
        "the import after the quote character is still reachable"
    );
    assert!(
        facts
            .declarations
            .iter()
            .any(|item| item.name == "classify"),
        "the lifetime is punctuation, not an unterminated literal"
    );
}

#[test]
fn a_restricted_visibility_still_leads_to_the_import() {
    assert_eq!(
        extract("pub(crate) use transport::serve_stdio;\n", Language::Rust)
            .imports
            .len(),
        1,
        "the parenthesised scope after pub must not hide the use"
    );
}

#[test]
fn a_grouped_use_names_the_module_without_its_separator() {
    let import = extract("use super::support::{one as first, two};\n", Language::Rust)
        .imports
        .remove(0);
    assert_eq!(import.specifier, "super::support");
    assert_eq!(import.names, ["first", "two"]);
    assert_eq!(
        import.bindings,
        [
            ImportBinding {
                imported: "one".to_owned(),
                local: "first".to_owned(),
            },
            ImportBinding {
                imported: "two".to_owned(),
                local: "two".to_owned(),
            },
        ]
    );
}

#[test]
fn rust_declarations_carry_their_kind_and_visibility() {
    let facts = extract(
        "pub struct Engine;\npub(crate) fn build() {}\nfn private() {}\ntrait Run {}\n",
        Language::Rust,
    );
    let items = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.exported))
        .collect::<Vec<_>>();
    assert!(
        items.contains(&("Engine", DeclarationKind::Struct, true)),
        "got {items:?}"
    );
    assert!(
        items.contains(&("build", DeclarationKind::Function, true)),
        "got {items:?}"
    );
    assert!(
        items.contains(&("private", DeclarationKind::Function, false)),
        "got {items:?}"
    );
    assert!(
        items.contains(&("Run", DeclarationKind::Trait, true)),
        "got {items:?}"
    );
}

#[test]
fn rust_function_pointer_types_do_not_invent_declarations_or_calls() {
    let facts = extract(
        "type Callback = unsafe extern \"system\" fn(\n\
         \x20   *mut c_void,\n\
         \x20   *const u16,\n\
         ) -> *mut c_void;\n",
        Language::Rust,
    );
    assert!(
        facts
            .declarations
            .iter()
            .any(|item| item.name == "Callback" && item.kind == DeclarationKind::TypeAlias),
        "the actual alias must survive, got {:?}",
        facts.declarations
    );
    for false_positive in ["mut", "const", "u16", "c_void"] {
        assert!(
            !facts
                .declarations
                .iter()
                .any(|item| item.name == false_positive),
            "{false_positive} is part of a pointer type, got {:?}",
            facts.declarations
        );
    }
    assert!(
        !facts
            .references
            .iter()
            .any(|reference| reference.name == "fn" && reference.kind == ReferenceKind::Call),
        "the function-pointer marker is a type, not a call"
    );
}
