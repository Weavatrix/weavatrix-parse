use super::extract;
use crate::facts::{DeclarationKind, ImportBinding};

#[test]
fn methods_belong_to_their_class_and_indentation_closes_scopes() {
    let source = "class Service:\n\
         \x20   def run(self):\n\
         \x20       return self.helper()\n\
         \x20   def helper(self):\n\
         \x20       return 1\n\
         \n\
         def module_level():\n\
         \x20   return Service()\n";
    let facts = extract(source);
    let declared = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.kind, item.owner.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        declared,
        [
            ("Service", DeclarationKind::Class, None),
            ("run", DeclarationKind::Method, Some("Service")),
            ("helper", DeclarationKind::Method, Some("Service")),
            ("module_level", DeclarationKind::Function, None),
        ],
        "dedenting to column one leaves the class"
    );
}

#[test]
fn a_docstring_is_text_even_when_it_looks_like_code() {
    let source = "def real():\n\
         \x20   \"\"\"\n\
         \x20   def fake():\n\
         \x20       import nothing\n\
         \x20   \"\"\"\n\
         \x20   return 1\n";
    let facts = extract(source);
    assert_eq!(
        facts
            .declarations
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["real"],
        "the definition inside the docstring is not a declaration"
    );
    assert!(
        facts.imports.is_empty(),
        "the import inside the docstring is not a dependency"
    );
}

#[test]
fn reads_the_import_forms_python_writes() {
    let source = "import os\n\
         import pkg.module\n\
         import json, time\n\
         import numpy as np\n\
         from .relative import thing as local_thing\n\
         from ..parent.pkg import other\n";
    let imports = extract(source).imports;
    let specifiers = imports
        .iter()
        .map(|import| import.specifier.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        specifiers,
        [
            "os",
            "pkg.module",
            "json",
            "time",
            "numpy",
            ".relative",
            "..parent.pkg",
        ]
    );
    let numpy = imports
        .iter()
        .find(|import| import.specifier == "numpy")
        .expect("numpy import");
    assert_eq!(numpy.names, ["np"]);
    assert_eq!(
        numpy.bindings,
        [ImportBinding {
            imported: "numpy".to_owned(),
            local: "np".to_owned(),
        }]
    );
    let relative = imports
        .iter()
        .find(|import| import.specifier == ".relative")
        .expect("relative import");
    assert_eq!(relative.names, ["local_thing"]);
    assert_eq!(
        relative.bindings,
        [ImportBinding {
            imported: "thing".to_owned(),
            local: "local_thing".to_owned(),
        }]
    );
}

#[test]
fn underscore_names_are_not_exported() {
    let facts = extract("def public():\n    pass\ndef _private():\n    pass\n");
    let exported = facts
        .declarations
        .iter()
        .map(|item| (item.name.as_str(), item.exported))
        .collect::<Vec<_>>();
    assert_eq!(exported, [("public", true), ("_private", false)]);
}

#[test]
fn f_string_expressions_keep_calls_and_ignore_literal_text() {
    let facts = extract(
        "def path(selector):\n    return f\"/{resolve_target(selector)} resolve_target {{literal}}\"\n",
    );
    let calls = facts
        .references
        .iter()
        .filter(|reference| reference.kind == crate::facts::ReferenceKind::Call)
        .collect::<Vec<_>>();

    assert_eq!(calls.len(), 1, "only executable interpolation is a call");
    assert_eq!(calls[0].name, "resolve_target");
    assert_eq!(calls[0].owner.as_deref(), Some("path"));
    assert_eq!(calls[0].span.line, 2);
}
