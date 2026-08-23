use super::*;

#[test]
fn swift_colon_heritage_splits_superclass_from_protocols() {
    let source = "import Foundation\n\
         final class RelayClient: NSObject, URLSessionWebSocketDelegate {\n\
         \x20 func connect() {}\n\
         }\n\
         struct Pairing: Equatable, Codable {}\n";
    let facts = extract(source, Language::Swift);
    let inherits = facts
        .references
        .iter()
        .filter(|item| item.kind == ReferenceKind::Inherits)
        .map(|item| (item.owner.as_deref(), item.name.as_str()))
        .collect::<Vec<_>>();
    let implements = facts
        .references
        .iter()
        .filter(|item| item.kind == ReferenceKind::Implements)
        .map(|item| (item.owner.as_deref(), item.name.as_str()))
        .collect::<Vec<_>>();
    assert!(
        inherits.contains(&(Some("RelayClient"), "NSObject")),
        "the first colon type on a class inherits, got {inherits:?}"
    );
    assert!(
        implements.contains(&(Some("RelayClient"), "URLSessionWebSocketDelegate")),
        "later colon types are protocols, got {implements:?}"
    );
    assert!(
        implements.contains(&(Some("Pairing"), "Equatable"))
            && implements.contains(&(Some("Pairing"), "Codable")),
        "a struct colon list is all conformance, got {implements:?}"
    );
}

#[test]
fn swift_parameter_colons_are_not_heritage() {
    let source = "func connect(pairing: Pairing, path: String) { open(path) }\n";
    let facts = extract(source, Language::Swift);
    assert!(
        facts
            .references
            .iter()
            .all(|item| item.kind != ReferenceKind::Inherits
                && item.kind != ReferenceKind::Implements),
        "pairing: Pairing is a parameter, got {:?}",
        facts.references
    );
}

#[test]
fn swift_generic_constraint_is_not_heritage() {
    let source = "class Box<T: Equatable>: Storage {\n\
         \x20 func get() -> T? { nil }\n\
         }\n";
    let facts = extract(source, Language::Swift);
    let names = facts
        .references
        .iter()
        .filter(|item| {
            item.kind == ReferenceKind::Inherits || item.kind == ReferenceKind::Implements
        })
        .map(|item| item.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Storage"], "T: Equatable is a bound, got {names:?}");
}

#[test]
fn swift_extension_records_the_protocols_it_satisfies() {
    let source = "extension Engine: Equatable, Codable {\n\
         \x20 func restart() {}\n\
         }\n";
    let facts = extract(source, Language::Swift);
    let protocols = facts
        .references
        .iter()
        .filter(|item| item.kind == ReferenceKind::Implements)
        .map(|item| item.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(protocols, ["Equatable", "Codable"]);
}

#[test]
fn swift_interpolated_url_keeps_the_static_route() {
    let source = "func open(base: String, mailbox: String) {\n\
         \x20 _ = URL(string: \"\\(base)/pair/\\(mailbox)\")\n\
         }\n";
    let facts = extract(source, Language::Swift);
    let url = facts
        .references
        .iter()
        .find(|item| item.name == "URL")
        .expect("URL call");
    assert!(
        url.string_arguments.iter().any(|item| item == "/pair"),
        "the static /pair segment must survive interpolation, got {:?}",
        url.string_arguments
    );
}

#[test]
fn swift_path_and_method_assignments_are_call_facts() {
    let source = "func open() {\n\
         \x20 comps.path = \"/ws\"\n\
         \x20 request.httpMethod = \"PUT\"\n\
         }\n";
    let facts = extract(source, Language::Swift);
    let path = facts
        .references
        .iter()
        .find(|item| item.name == "path")
        .expect("path assignment");
    assert_eq!(path.string_arguments, ["/ws"]);
    let method = facts
        .references
        .iter()
        .find(|item| item.name == "httpMethod")
        .expect("httpMethod assignment");
    assert_eq!(method.string_arguments, ["PUT"]);
}
