use super::*;

#[test]
fn go_groups_imports_and_capitalisation_marks_export() {
    let source = "package main\n\nimport (\n\tf \"fmt\"\n\t\"edgehawk.com/app/reader\"\n)\n\n\
         func Exported() {}\nfunc internal() {}\n";
    let facts = extract(source, Language::Go);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        ["fmt", "edgehawk.com/app/reader"],
        "a grouped import block yields one fact per path"
    );
    assert_eq!(
        facts.imports[0].bindings,
        [ImportBinding {
            imported: "fmt".to_owned(),
            local: "f".to_owned(),
        }]
    );
    assert_eq!(
        facts.imports[1].bindings,
        [ImportBinding {
            imported: "reader".to_owned(),
            local: "reader".to_owned(),
        }]
    );
    let items = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.exported))
        .collect::<Vec<_>>();
    assert!(items.contains(&("Exported", true)), "got {items:?}");
    assert!(items.contains(&("internal", false)), "got {items:?}");
}

#[test]
fn go_const_and_var_groups_declare_each_line() {
    let source = r#"package main
const (
EventAdd = "added"
eventDelete = "deleted"
)
var (
endpoint = flag.String("endpoint", "/events", "endpoint")
topics = []string{EventAdd, eventDelete}
)
"#;
    let facts = extract(source, Language::Go);
    let items = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.span.line))
        .collect::<Vec<_>>();
    for expected in [
        ("EventAdd", DeclarationKind::Constant, 3),
        ("eventDelete", DeclarationKind::Constant, 4),
        ("endpoint", DeclarationKind::Variable, 7),
        ("topics", DeclarationKind::Variable, 8),
    ] {
        assert!(
            items.contains(&expected),
            "missing {expected:?}; got {items:?}"
        );
    }
}

#[test]
fn go_grouped_initializers_keep_their_call_references() {
    let facts = extract(
        "package main\nvar (\n flagName = config.String(\"name\")\n)\n",
        Language::Go,
    );
    assert!(
        facts
            .declarations
            .iter()
            .any(|item| item.name == "flagName" && item.kind == DeclarationKind::Variable),
        "the grouped declaration must survive"
    );
    assert!(
        facts.references.iter().any(|reference| {
            reference.name == "String"
                && reference.kind == ReferenceKind::Call
                && reference.receiver.as_deref() == Some("config")
        }),
        "the initializer call must survive, got {:?}",
        facts.references
    );
}

#[test]
fn go_grouped_values_do_not_turn_continuation_lines_into_declarations() {
    let source = r#"package main
var (
config = Config{
    Name: "primary",
}
continued =
    buildValue
next = 1
)
"#;
    let facts = extract(source, Language::Go);
    let names = facts
        .declarations
        .iter()
        .map(|item| item.name.as_str())
        .collect::<Vec<_>>();
    for expected in ["config", "continued", "next"] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }
    for false_positive in ["Name", "buildValue"] {
        assert!(
            !names.contains(&false_positive),
            "{false_positive} is an initializer expression, got {names:?}"
        );
    }
}

#[test]
fn a_large_go_group_does_not_swallow_following_functions() {
    use std::fmt::Write as _;

    let mut source = String::from("package main\nvar (\n");
    for index in 0..1_100 {
        let _ = writeln!(source, "value{index} = {index}");
    }
    source.push_str(")\nfunc AfterGroup() {}\n");
    let facts = extract(&source, Language::Go);
    assert!(
        facts
            .declarations
            .iter()
            .any(|item| item.name == "AfterGroup" && item.kind == DeclarationKind::Function),
        "the closing group delimiter must return scanning to the following function"
    );
}
