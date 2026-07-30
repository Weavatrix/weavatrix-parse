use super::*;

#[test]
fn nested_template_text_never_becomes_a_call() {
    let source = r"function exactUsage(files) {
  return `${files ? ` in ${plural(files)} file(s)` : ''}`
}
";
    let facts = extract(source, Language::JavaScript);
    assert!(
        !facts
            .references
            .iter()
            .any(|reference| reference.name == "file"),
        "`file(s)` is literal template text, got {:?}",
        facts.references
    );
    let plural = facts
        .references
        .iter()
        .find(|reference| reference.name == "plural")
        .expect("call in a nested interpolation");
    assert_eq!(plural.kind, ReferenceKind::Call);
    assert_eq!(plural.owner.as_deref(), Some("exactUsage"));
    assert_eq!(plural.span.line, 2);
}

#[test]
fn template_interpolations_keep_all_calls_and_exact_spans() {
    let source = r"function describe(blob, name, edge, graph) {
  const mentioned = new RegExp(`x${escRe(name)}y`).test(blob)
  return `${compileKind(edge) ? labelOf(graph, edge.id) : ''}`
}
";
    let facts = extract(source, Language::JavaScript);
    let calls = facts
        .references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Call)
        .collect::<Vec<_>>();
    for (name, line) in [
        ("RegExp", 2),
        ("escRe", 2),
        ("test", 2),
        ("compileKind", 3),
        ("labelOf", 3),
    ] {
        let reference = calls
            .iter()
            .find(|reference| reference.name == name)
            .unwrap_or_else(|| panic!("missing {name}, got {calls:?}"));
        assert_eq!(reference.span.line, line);
        assert_eq!(
            &source[reference.span.start..reference.span.end],
            name,
            "relocated span must name the original call"
        );
        assert_eq!(reference.owner.as_deref(), Some("describe"));
    }
}
