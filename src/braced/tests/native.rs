use super::*;

#[test]
fn a_rust_impl_gives_its_methods_an_owner() {
    let source = "struct Engine;\n\
         impl Engine {\n\
         \x20   pub fn start(&self) {}\n\
         }\n\
         impl Display for Engine {\n\
         \x20   fn fmt(&self) {}\n\
         }\n";
    let items = declared(source, Language::Rust);
    for method in ["start", "fmt"] {
        assert!(
            items
                .iter()
                .any(|(name, _, owner)| name == method && owner.as_deref() == Some("Engine")),
            "{method} belongs to Engine, not to the trait, got {items:?}"
        );
    }
}

#[test]
fn c_functions_are_declarations_rather_than_calls_to_themselves() {
    // No keyword introduces a C function, so every definition fell through
    // to the call path: the graph gained a self-edge and lost the
    // declaration that dead-code analysis looks for.
    let source = "#include <stdio.h>\n\
         int add(int a, int b) { return a + b; }\n\
         static void run(void) { add(1, 2); }\n\
         int main(void) { run(); return 0; }\n";
    let facts = extract(source, Language::C);
    let declared = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.exported))
        .collect::<Vec<_>>();
    assert_eq!(
        declared,
        [
            ("add", DeclarationKind::Function, true),
            ("run", DeclarationKind::Function, false),
            ("main", DeclarationKind::Function, true),
        ],
        "a static function is file-local; the rest are linkable"
    );
    assert_eq!(
        facts
            .references
            .iter()
            .map(|reference| reference.name.as_str())
            .collect::<Vec<_>>(),
        ["add", "run"],
        "only the two real call sites, and no definition among them"
    );
}

#[test]
fn an_include_does_not_swallow_the_line_beneath_it() {
    let facts = extract(
        "#include <stdio.h>\nint first(void) { return 0; }\n",
        Language::C,
    );
    assert_eq!(
        facts.imports.len(),
        1,
        "the include is one dependency, not a run-on statement"
    );
    assert!(
        facts.declarations.iter().any(|item| item.name == "first"),
        "the function under the include survives, got {:?}",
        facts.declarations
    );
}

#[test]
fn an_out_of_line_cpp_definition_belongs_to_its_class() {
    let facts = extract(
        "int helper(int x) { return x; }\nvoid Engine::start() { helper(1); }\n",
        Language::Cpp,
    );
    let start = facts
        .declarations
        .iter()
        .find(|item| item.name == "start")
        .expect("the qualified definition is a declaration");
    assert_eq!(start.kind, DeclarationKind::Method);
    assert_eq!(start.owner.as_deref(), Some("Engine"));
}

#[test]
fn a_type_argument_reaches_the_name_a_mapper_configures() {
    // An object-relational mapper names the entity in the type argument
    // and the table in the string one, so a framework layer needs both to
    // connect them - and the parenthesis does not follow the name.
    let facts = extract(
        "modelBuilder.Entity<Order>().ToTable(\"orders\");\nif (a < b && c > d) {}\n",
        Language::CSharp,
    );
    let seen = facts
        .references
        .iter()
        .map(|reference| {
            (
                reference.name.as_str(),
                reference.kind,
                reference.string_arguments.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        seen.contains(&("Order", ReferenceKind::Uses, Vec::new())),
        "the entity type is reachable, got {seen:?}"
    );
    assert!(
        seen.contains(&("ToTable", ReferenceKind::Call, vec!["orders".to_owned()])),
        "and so is the table it maps to, got {seen:?}"
    );
    assert!(
        !seen.iter().any(|(name, ..)| *name == "b"),
        "a comparison is not a type argument list, got {seen:?}"
    );
}

#[test]
fn a_control_structure_is_not_a_function_definition() {
    let source = "int pick(int x) {\n\
         \x20 if (x) { return 1; }\n\
         \x20 else if (x > 2) { return 2; }\n\
         \x20 while (x) { x--; }\n\
         \x20 return helper(x);\n\
         }\n";
    let names = declared(source, Language::C)
        .into_iter()
        .map(|(name, ..)| name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["pick"],
        "an else-if has an identifier before it and still declares nothing"
    );
}
