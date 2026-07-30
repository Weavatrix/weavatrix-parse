use super::*;

#[test]
fn java_methods_belong_to_their_class() {
    let source = "package com.x;\nimport com.x.Helper;\n\
         public class Service {\n\
         \x20 private final Helper helper = null;\n\
         \x20 public void run() {\n\
         \x20   items.forEach(item -> {});\n\
         \x20 }\n\
         \x20 private int score(String value) { return 1; }\n\
         }\n";
    let items = declared(source, Language::Java);
    assert!(
        items.iter().any(|(name, kind, owner)| name == "Service"
            && *kind == DeclarationKind::Class
            && owner.is_none()),
        "got {items:?}"
    );
    for method in ["run", "score"] {
        assert!(
            items.iter().any(|(name, kind, owner)| name == method
                && *kind == DeclarationKind::Method
                && owner.as_deref() == Some("Service")),
            "{method} must be a method of Service, got {items:?}"
        );
    }
    assert!(
        !items.iter().any(|(name, ..)| name == "forEach"),
        "a call chain inside a body is not a declaration, got {items:?}"
    );
}

#[test]
fn java_route_annotations_keep_their_structural_owner_and_literal() {
    let source = "@RequestMapping(\"warehouse\")\n\
         public class Service {\n\
         \x20 @GetMapping(\"/stock\")\n\
         \x20 public void stock() {}\n\
         }\n";
    let facts = extract(source, Language::Java);
    let class_mapping = facts
        .calls()
        .find(|call| call.name == "RequestMapping")
        .expect("class mapping call");
    assert_eq!(class_mapping.owner, None);
    assert_eq!(class_mapping.string_arguments, ["warehouse"]);

    let method_mapping = facts
        .calls()
        .find(|call| call.name == "GetMapping")
        .expect("method mapping call");
    assert_eq!(method_mapping.owner.as_deref(), Some("Service"));
    assert_eq!(method_mapping.string_arguments, ["/stock"]);
}

#[test]
fn solidity_contracts_own_their_functions_and_name_their_dependencies() {
    let source = "// SPDX-License-Identifier: MIT\n\
         pragma solidity ^0.8.20;\n\
         import \"./Ownable.sol\";\n\
         import {IERC20, SafeMath} from \"@openzeppelin/contracts/token/IERC20.sol\";\n\
         \n\
         contract Vault is Ownable {\n\
         \x20 event Deposited(address indexed who, uint256 amount);\n\
         \x20 function deposit(uint256 amount) public payable {\n\
         \x20   token.transferFrom(msg.sender, address(this), amount);\n\
         \x20 }\n\
         \x20 function _sweep() internal {}\n\
         }\n";
    let facts = extract(source, Language::Solidity);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        ["./Ownable.sol", "@openzeppelin/contracts/token/IERC20.sol"],
        "the names listed before `from` are bindings, not the path"
    );
    let items = declared(source, Language::Solidity);
    assert!(
        items.iter().any(|(name, kind, owner)| name == "Vault"
            && *kind == DeclarationKind::Class
            && owner.is_none()),
        "got {items:?}"
    );
    for method in ["deposit", "_sweep"] {
        assert!(
            items.iter().any(|(name, kind, owner)| name == method
                && *kind == DeclarationKind::Function
                && owner.as_deref() == Some("Vault")),
            "{method} belongs to Vault, got {items:?}"
        );
    }
    assert!(
        facts
            .references
            .iter()
            .any(|call| call.name == "transferFrom" && call.receiver.as_deref() == Some("token")),
        "a call through a receiver keeps the receiver"
    );
}

#[test]
fn swift_members_belong_to_the_type_their_extension_names() {
    let source = "import Foundation\n\
         import UIKit\n\
         \n\
         public struct Engine {\n\
         \x20 let name: String\n\
         \x20 public func start() { boot() }\n\
         }\n\
         \n\
         extension Engine {\n\
         \x20 func restart() { start() }\n\
         }\n\
         \n\
         private func boot() {}\n";
    let facts = extract(source, Language::Swift);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        ["Foundation", "UIKit"]
    );
    let items = declared(source, Language::Swift);
    assert!(
        items.iter().any(|(name, kind, owner)| name == "start"
            && *kind == DeclarationKind::Function
            && owner.as_deref() == Some("Engine")),
        "got {items:?}"
    );
    assert!(
        items
            .iter()
            .any(|(name, _, owner)| name == "restart" && owner.as_deref() == Some("Engine")),
        "an extension names what its members belong to, got {items:?}"
    );
    assert!(
        !items.iter().any(|(name, ..)| name == "extension"),
        "and declares nothing itself, got {items:?}"
    );
    assert!(
        items
            .iter()
            .any(|(name, _, owner)| name == "boot" && owner.is_none()),
        "the file-level function is back outside, got {items:?}"
    );
}
