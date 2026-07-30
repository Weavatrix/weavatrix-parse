use super::*;

#[test]
fn extracts_typed_proto3_rpc_and_streaming_with_exact_spans() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "package shop.v1;\n",
        "message Outer { message Request {} }\n",
        "message Reply {}\n",
        "service Inventory { rpc Watch(stream Outer.Request) returns (stream Reply); }\n",
    );
    let facts = extract(source);
    assert!(facts.diagnostics.is_empty());
    assert!(
        !facts.contracts.is_empty(),
        "advertised valid proto3 cannot fall back to empty facts"
    );
    assert!(facts.contracts.iter().any(|fact| {
        matches!(
            fact.kind,
            ContractKind::ProtobufRpc {
                ref input,
                ref output,
                client_streaming: true,
                server_streaming: true,
            } if input == "Outer.Request" && output == "Reply"
        ) && &source[fact.span.start..fact.span.end] == "Watch"
    }));
    assert!(facts.contracts.iter().any(|fact| {
        fact.name == "Outer.Request"
            && fact.owner.as_deref() == Some("Outer")
            && fact.kind == ContractKind::ProtobufMessage
    }));
}

#[test]
fn extracts_proto2_and_supported_editions_service_contracts() {
    for (source, option_import) in [
        (
            concat!(
                "syntax = \"proto2\";\n",
                "message Request { optional string id = 1; }\n",
                "message Reply {}\n",
                "service Legacy { rpc Get(Request) returns (Reply); }\n",
            ),
            None,
        ),
        (
            concat!(
                "edition = \"2023\";\n",
                "message Request { string id = 1; }\n",
                "message Reply {}\n",
                "service EditionApi { rpc Get(Request) returns (Reply); }\n",
            ),
            None,
        ),
        (
            concat!(
                "edition = \"2024\";\n",
                "import option \"custom_options.proto\";\n",
                "message Request { string id = 1; }\n",
                "message Reply {}\n",
                "service EditionApi { rpc Get(Request) returns (Reply); }\n",
            ),
            Some("custom_options.proto"),
        ),
    ] {
        let facts = extract(source);
        assert!(facts.diagnostics.is_empty(), "{facts:?}");
        assert!(facts.contracts.iter().any(|fact| {
            matches!(fact.kind, ContractKind::ProtobufRpc { .. }) && fact.name == "Get"
        }));
        if let Some(option_import) = option_import {
            let import = facts
                .imports
                .iter()
                .find(|import| import.specifier == option_import)
                .expect("Edition 2024 option import");
            assert!(!import.reexport);
            assert_eq!(
                source[import.span.start..import.span.end].trim_matches(['"', '\'']),
                option_import
            );
        }
    }
}

#[test]
fn invalid_dialects_fail_closed_with_exact_diagnostics() {
    for source in [
        "syntax = \"proto1\"; message Legacy {}",
        "edition = \"future\"; message Legacy {}",
        "edition = \"2022\"; message Legacy {}",
        "edition = \"2026\"; message Legacy {}",
        "package misplaced; syntax = \"proto3\"; message Legacy {}",
        "syntax = \"proto3\"; edition = \"2023\"; message Legacy {}",
    ] {
        let facts = extract(source);
        assert!(facts.contracts.is_empty());
        assert_eq!(facts.diagnostics.len(), 1);
        assert_eq!(facts.diagnostics[0].code, "protobuf.invalid_dialect");
        let span = facts.diagnostics[0].span;
        assert_eq!((span.line, span.column), (1, 1));
        assert!(!source[span.start..span.end].is_empty());
    }
}

#[test]
fn malformed_supported_dialects_fail_closed() {
    for source in [
        "syntax = \"proto3\"; service Broken {",
        "syntax = \"proto3\"; package bad message X {};",
        "syntax = \"proto3\"; service Bad { rpc Call(Request) (Reply); }",
    ] {
        let facts = extract(source);
        assert!(facts.contracts.is_empty());
        assert_eq!(facts.diagnostics.len(), 1);
        assert_eq!(facts.diagnostics[0].code, "protobuf.syntax_error");
        let span = facts.diagnostics[0].span;
        assert!(
            span.end > span.start,
            "diagnostic must identify the exact offending token"
        );
        assert!(!source[span.start..span.end].is_empty());
    }
}
