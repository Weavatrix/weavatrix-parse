use super::*;

#[test]
fn exported_function_extent_includes_its_body() {
    let source = "export function canDelete(viewer) {\n  return !viewer;\n}\n";
    let facts = extract(source, Language::JavaScript);
    let declaration = facts
        .declarations
        .iter()
        .find(|item| item.name == "canDelete")
        .expect("the exported function is declared");

    assert_eq!(
        &source[declaration.extent.start..declaration.extent.end],
        "export function canDelete(viewer) {\n  return !viewer;\n}"
    );
}

#[test]
fn class_bodies_yield_methods_and_fields_with_their_owner() {
    let source = "export class Service {\n\
         \x20 private cache = new Map();\n\
         \x20 readonly limit: number = 10;\n\
         \x20 async run(input: string) {\n\
         \x20   return this.helper(input);\n\
         \x20 }\n\
         \x20 helper(value: string) { return value; }\n\
         }\n";
    let facts = extract(source, Language::TypeScript);
    let declared = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.owner.as_deref()))
        .collect::<Vec<_>>();
    assert!(
        declared.contains(&("Service", DeclarationKind::Class, None)),
        "got {declared:?}"
    );
    assert!(
        declared.contains(&("run", DeclarationKind::Method, Some("Service"))),
        "a class method is a declaration owned by its class, got {declared:?}"
    );
    assert!(
        declared.contains(&("helper", DeclarationKind::Method, Some("Service"))),
        "got {declared:?}"
    );
    assert!(
        declared.contains(&("cache", DeclarationKind::Field, Some("Service"))),
        "got {declared:?}"
    );
    let call = facts
        .references
        .iter()
        .find(|call| call.name == "helper")
        .expect("the call inside run is recorded");
    assert_eq!(call.receiver.as_deref(), Some("this"));
    assert_eq!(call.owner.as_deref(), Some("run"));
}

#[test]
fn arrow_constants_are_functions_and_plain_constants_are_not() {
    let source = "export const load = async () => { return 1; };\n\
         const multiline =\n(value) => value;\n\
         const limit = 10;\n";
    let facts = extract(source, Language::TypeScript);
    let kinds = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.exported))
        .collect::<Vec<_>>();
    assert!(
        kinds.contains(&("load", DeclarationKind::Function, true)),
        "got {kinds:?}"
    );
    assert!(
        kinds.contains(&("multiline", DeclarationKind::Function, false)),
        "got {kinds:?}"
    );
    assert!(
        kinds.contains(&("limit", DeclarationKind::Constant, false)),
        "got {kinds:?}"
    );
}

#[test]
fn regexes_and_collection_initializers_are_not_arrow_functions() {
    let source = "const SAFE_SCRIPT = /^(?:test(?::|$)|[^:]+:(?:test|check)(?::|$))/i\n\
         const UNSAFE_SHELL_ARG = /[\\0\\r\\n&|<>^%!`\\\"]/ \n\
         const byId = new Map((graph.nodes || []).map((node) => [String(node.id), node]))\n\
         const files = new Set((graph.nodes || []).filter((node) => node.id))\n\
         const adjacency = new Map([...files].map((file) => [file, new Set()]))\n";
    let facts = extract(source, Language::JavaScript);
    for name in [
        "SAFE_SCRIPT",
        "UNSAFE_SHELL_ARG",
        "byId",
        "files",
        "adjacency",
    ] {
        let declaration = facts
            .declarations
            .iter()
            .find(|item| item.name == name)
            .unwrap_or_else(|| panic!("missing {name}: {facts:?}"));
        assert_eq!(
            declaration.kind,
            DeclarationKind::Constant,
            "{name} is a value initializer"
        );
    }
}

#[test]
fn exported_functions_and_returned_object_methods_are_declarations() {
    let source = "export function runCommand(command, args = [], options = {}) {}\n\
         export function createGate() {\n\
         \x20 return {\n\
         \x20   shouldShow({ force = false } = {}) { return force },\n\
         \x20   reset() {},\n\
         \x20 }\n\
         }\n\
         export function createClassifier() {\n\
         \x20 return { explain(path, options = {}) { return path } }\n\
         }\n";
    let facts = extract(source, Language::JavaScript);
    let declared = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.owner.as_deref()))
        .collect::<Vec<_>>();
    assert!(
        declared.contains(&("runCommand", DeclarationKind::Function, None)),
        "got {declared:?}"
    );
    for (name, owner) in [
        ("shouldShow", "createGate"),
        ("reset", "createGate"),
        ("explain", "createClassifier"),
    ] {
        assert!(
            declared.contains(&(name, DeclarationKind::Method, Some(owner))),
            "missing {owner}.{name}; got {declared:?}"
        );
    }
}

#[test]
fn exporting_an_imported_binding_keeps_its_origin() {
    let source = "import { safeRead, MAX_FILE_BYTES } from '../util.js';\nexport { safeRead };\n";
    let facts = extract(source, Language::JavaScript);
    let forwarded = facts
        .imports
        .iter()
        .find(|item| item.reexport)
        .expect("local export of imported binding");
    assert_eq!(forwarded.specifier, "../util.js");
    assert_eq!(forwarded.names, ["safeRead"]);
    assert_eq!(
        forwarded.bindings,
        [ImportBinding {
            imported: "safeRead".to_owned(),
            local: "safeRead".to_owned(),
        }]
    );
}

#[test]
fn aliased_imports_preserve_original_and_local_names() {
    let facts = extract(
        "import Default, {\n\
         \x20 architectureViolation as violation,\n\
         \x20 matchComponentSelector as matches,\n\
         } from './architecture.js';\n\
         import * as catalog from './catalog.js';\n",
        Language::JavaScript,
    );
    let architecture = facts
        .imports
        .iter()
        .find(|item| item.specifier == "./architecture.js")
        .expect("architecture import");
    assert_eq!(architecture.names, ["Default", "violation", "matches"]);
    assert_eq!(
        architecture.bindings,
        [
            ImportBinding {
                imported: "default".to_owned(),
                local: "Default".to_owned(),
            },
            ImportBinding {
                imported: "architectureViolation".to_owned(),
                local: "violation".to_owned(),
            },
            ImportBinding {
                imported: "matchComponentSelector".to_owned(),
                local: "matches".to_owned(),
            },
        ]
    );
    let catalog = facts
        .imports
        .iter()
        .find(|item| item.specifier == "./catalog.js")
        .expect("namespace import");
    assert_eq!(
        catalog.bindings,
        [ImportBinding {
            imported: "*".to_owned(),
            local: "catalog".to_owned(),
        }]
    );
}

#[test]
fn typescript_di_wiring_is_use_reference_evidence() {
    let source = "import { Controller, Get } from '@nestjs/common';\n\
         import { UsersService } from './users.service';\n\
         @Controller('users')\n\
         export class UsersController {\n\
           private readonly registry: AuditRegistry;\n\
           constructor(private readonly usersService: UsersService) {}\n\
           @Get()\n\
           findAll(): string[] {\n\
             return this.usersService.findAll();\n\
           }\n\
         }\n";
    let facts = extract(source, Language::TypeScript);
    let uses = facts
        .references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Uses)
        .map(|reference| (reference.name.clone(), reference.owner.clone()))
        .collect::<Vec<_>>();

    assert!(
        uses.iter()
            .any(|(name, owner)| name == "UsersService"
                && owner.as_deref() == Some("UsersController")),
        "constructor parameter type wires UsersService, got {uses:?}"
    );
    assert!(
        uses.iter()
            .any(|(name, owner)| name == "AuditRegistry"
                && owner.as_deref() == Some("UsersController")),
        "field annotation type wires AuditRegistry, got {uses:?}"
    );
    assert!(
        !uses
            .iter()
            .any(|(name, _)| name == "usersService" || name == "Controller"),
        "parameter names and decorators are not type uses: {uses:?}"
    );
}
