use super::*;

#[test]
fn nested_call_arguments_are_not_object_methods() {
    let source = r"function build(makeClient, session) {
  return {
make: () => makeClient({ timeoutMs: Math.max(100, remaining(session)) }),
  }
}
";
    let facts = extract(source, Language::JavaScript);
    let remaining = facts
        .references
        .iter()
        .filter(|reference| reference.name == "remaining" && reference.kind == ReferenceKind::Call)
        .collect::<Vec<_>>();
    assert_eq!(remaining.len(), 1, "got {:?}", facts.references);
    assert_eq!(remaining[0].span.line, 3);
    assert!(
        !facts.declarations.iter().any(|declaration| {
            declaration.name == "remaining" && declaration.kind == DeclarationKind::Method
        }),
        "a nested argument is not an object method: {:?}",
        facts.declarations
    );
}

#[test]
fn default_object_parameter_is_not_the_function_body() {
    let source = r"export function runCommand(command, args = [], options = {}) {
  return spawn(command, args, { env: childProcessEnv(options.env || {}) })
}
";
    let facts = extract(source, Language::JavaScript);
    let declaration = facts
        .declarations
        .iter()
        .find(|declaration| declaration.name == "runCommand")
        .expect("exported function declaration");
    assert_eq!(declaration.kind, DeclarationKind::Function);
    assert!(declaration.exported);
    let environment = facts
        .references
        .iter()
        .find(|reference| reference.name == "childProcessEnv")
        .expect("call inside function body");
    assert_eq!(environment.owner.as_deref(), Some("runCommand"));
}

#[test]
fn call_in_returned_object_property_is_retained() {
    let source = r"function withGraph(graph) {
  const root = mkdtempSync('prefix')
  const graphPath = join(root, 'graph.json')
  return {root, graphPath, graph: loadGraph(graphPath)}
}
";
    let facts = extract(source, Language::JavaScript);
    let load = facts
        .references
        .iter()
        .find(|reference| reference.name == "loadGraph" && reference.kind == ReferenceKind::Call)
        .unwrap_or_else(|| panic!("missing loadGraph, got {facts:?}"));
    assert_eq!(load.owner.as_deref(), Some("withGraph"));
    assert_eq!(load.span.line, 4);
}

#[test]
fn object_method_names_are_not_calls_but_their_bodies_are() {
    let source = r"function wrap(client) {
  return Object.freeze({
fromUri(uri) { return client.normalizer.fromUri(uri) },
kill() { client.kill() },
  })
}
";
    let facts = extract(source, Language::JavaScript);
    let calls = facts
        .references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Call)
        .collect::<Vec<_>>();
    assert_eq!(
        calls
            .iter()
            .filter(|reference| reference.name == "fromUri")
            .count(),
        1,
        "only the body call is a reference, got {calls:?}"
    );
    let from_uri = calls
        .iter()
        .find(|reference| reference.name == "fromUri")
        .expect("body call");
    assert_eq!(from_uri.receiver.as_deref(), Some("normalizer"));
    assert_eq!(from_uri.span.line, 3);
    assert!(
        from_uri.span.column > 30,
        "the call must point inside the body, got {:?}",
        from_uri.span
    );
    assert_eq!(
        calls
            .iter()
            .filter(|reference| reference.name == "kill")
            .count(),
        1,
        "only client.kill() is a call, got {calls:?}"
    );
}

#[test]
fn typescript_generic_calls_keep_their_call_fact() {
    let facts = extract(
        "export async function loadUser() { return get<User>('/users/1'); }\n",
        Language::TypeScript,
    );
    let call = facts
        .references
        .iter()
        .find(|reference| reference.name == "get" && reference.kind == ReferenceKind::Call)
        .expect("generic call");
    assert_eq!(call.string_arguments, ["/users/1"]);
}

#[test]
fn calls_inside_object_fields_and_nested_arguments_are_all_retained() {
    let source = "function score(entry, count, total) {\n\
         \x20 return {...entry, hotspotScore: round(Math.sqrt(entry.value))}\n\
         }\n\
         function pair(pairs, count, total) {\n\
         \x20 pairs.push({jaccard: round(count / total), lift: round(Math.max(count, total))})\n\
         }\n";
    let facts = extract(source, Language::JavaScript);
    let calls = facts
        .references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Call)
        .map(|reference| (reference.name.as_str(), reference.span.line))
        .collect::<Vec<_>>();
    assert_eq!(
        calls.iter().filter(|(name, _)| *name == "round").count(),
        3,
        "every aliased round call must survive, got {calls:?}"
    );
    for expected in [
        ("round", 2),
        ("sqrt", 2),
        ("push", 5),
        ("round", 5),
        ("max", 5),
    ] {
        assert!(
            calls.contains(&expected),
            "missing {expected:?}; got {calls:?}"
        );
    }
    for false_method in ["round", "sqrt", "max"] {
        assert!(
            !facts.declarations.iter().any(|declaration| {
                declaration.name == false_method && declaration.kind == DeclarationKind::Method
            }),
            "a call used as an object value is not a method declaration"
        );
    }
}
