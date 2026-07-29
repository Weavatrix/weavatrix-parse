//! Throughput comparison against tree-sitter on the same source files.
//!
//! Two things are measured because they are two different jobs. `tokenize`
//! against `tree-sitter parse` compares the cost of turning bytes into a
//! traversable form. `extract` against `parse + walk` compares the cost of
//! answering the question a consumer actually asks: what does this file
//! declare, import and call. Reporting only the first would flatter whichever
//! side does less work.
//!
//! Usage: `competitor-bench [--output <path>] <corpus-dir>...`

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use tree_sitter::{Language as TsLanguage, Parser};
use weavatrix_parse::{
    ContractKind, DeclarationKind, GraphqlOperation, Language, Mode, Tokenizer, extract,
};

type LanguageGrammar = (Language, fn() -> TsLanguage);

/// How many measured rounds follow the warm-up; the median is reported.
const ROUNDS: usize = 7;

/// Largest corpus per language, so one huge vendored tree cannot dominate.
const CORPUS_LIMIT: usize = 8 * 1024 * 1024;

fn main() {
    let mut roots = env::args().skip(1).collect::<Vec<_>>();
    let output = take_option(&mut roots, "--output");
    let auditing = roots.iter().any(|argument| argument == "--audit");
    let verifying = roots
        .iter()
        .any(|argument| argument == "--verify-ground-truth");
    roots.retain(|argument| argument != "--audit" && argument != "--verify-ground-truth");
    if roots.is_empty() {
        eprintln!(
            "usage: competitor-bench [--audit|--verify-ground-truth] \
             [--output <path>] <corpus-dir>..."
        );
        std::process::exit(2);
    }

    let languages: &[LanguageGrammar] = &[
        (Language::JavaScript, || {
            tree_sitter_javascript::LANGUAGE.into()
        }),
        (Language::TypeScript, || {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        }),
        (Language::Python, || tree_sitter_python::LANGUAGE.into()),
        (Language::Rust, || tree_sitter_rust::LANGUAGE.into()),
        (Language::Go, || tree_sitter_go::LANGUAGE.into()),
        (Language::Java, || tree_sitter_java::LANGUAGE.into()),
        (Language::CSharp, || tree_sitter_c_sharp::LANGUAGE.into()),
        (Language::C, || tree_sitter_c::LANGUAGE.into()),
        (Language::Cpp, || tree_sitter_cpp::LANGUAGE.into()),
        // These are maintained community grammars. They are used for
        // parse+walk throughput only; typed contract correctness is established
        // by the exact source/span/kind fixtures below, not by node counts.
        (Language::Graphql, || tree_sitter_graphql::LANGUAGE.into()),
        (Language::Protobuf, || tree_sitter_proto::LANGUAGE.into()),
        // SQL has no first-party tree-sitter grammar; this is the maintained
        // community one, which is what a consumer would actually reach for.
        (Language::Sql, || tree_sitter_sequel::LANGUAGE.into()),
        (Language::Swift, || tree_sitter_swift::LANGUAGE.into()),
        (Language::Bash, || tree_sitter_bash::LANGUAGE.into()),
        (Language::Terraform, || tree_sitter_hcl::LANGUAGE.into()),
        (Language::Markdown, || tree_sitter_md::LANGUAGE.into()),
        (Language::Xml, || tree_sitter_xml::LANGUAGE_XML.into()),
    ];

    let corpora = collect_all(&roots, languages);
    if verifying {
        verify_ground_truth(&corpora, languages);
        return;
    }
    if auditing {
        audit(&corpora, languages);
        return;
    }

    let header = format!(
        "{:<12} {:>6} {:>7} {:>11} {:>11} {:>11} {:>11} {:>10}",
        "language", "files", "MB", "tokenize", "extract", "ts parse", "ts walk", "extract/ts"
    );
    let mut report = format!(
        "statistic=median measured_rounds={ROUNDS} warmup_rounds=1 \
         cap_bytes_per_language={CORPUS_LIMIT}\n{header}\n{}\n",
        "-".repeat(88)
    );
    print!("{report}");

    for (language, grammar) in languages {
        let Some(corpus) = corpora.get(language) else {
            continue;
        };
        if corpus.is_empty() {
            continue;
        }
        let bytes = corpus.iter().map(|(_, source)| source.len()).sum::<usize>();

        let mut parser = Parser::new();
        if parser.set_language(&grammar()).is_err() {
            eprintln!(
                "{}: grammar rejected by this tree-sitter",
                language.as_str()
            );
            continue;
        }

        // The four measurements are interleaved within each round rather than
        // run one after another. On a machine doing anything else, load drifts
        // over seconds, and consecutive blocks would charge that drift to
        // whichever implementation happened to run during it - which is how a
        // first run produced a `walk` faster than the `parse` it contains.
        let mut tokenize_samples = Vec::with_capacity(ROUNDS);
        let mut structure_samples = Vec::with_capacity(ROUNDS);
        let mut ts_parse_samples = Vec::with_capacity(ROUNDS);
        let mut ts_walk_samples = Vec::with_capacity(ROUNDS);
        for round in 0..=ROUNDS {
            let one = time(|| {
                corpus
                    .iter()
                    .map(|(_, source)| Tokenizer::new(source, *language).mode(Mode::Lite).count())
                    .sum::<usize>()
            });
            let two = time(|| {
                corpus
                    .iter()
                    .map(|(_, source)| {
                        let facts = extract(source, *language);
                        facts.declarations.len() + facts.imports.len() + facts.references.len()
                    })
                    .sum::<usize>()
            });
            let three = time(|| {
                let mut count = 0_usize;
                for (_, source) in corpus {
                    if let Some(tree) = parser.parse(source, None) {
                        count += tree.root_node().child_count();
                    }
                }
                count
            });
            let four = time(|| {
                let mut count = 0_usize;
                for (_, source) in corpus {
                    if let Some(tree) = parser.parse(source, None) {
                        count += walk(&tree);
                    }
                }
                count
            });
            // Round zero only warms caches and allocators.
            if round == 0 {
                continue;
            }
            tokenize_samples.push(one);
            structure_samples.push(two);
            ts_parse_samples.push(three);
            ts_walk_samples.push(four);
        }

        let tokenize = median(&mut tokenize_samples);
        let structure = median(&mut structure_samples);
        let ts_parse = median(&mut ts_parse_samples);
        let ts_walk = median(&mut ts_walk_samples);
        let line = format!(
            "{:<12} {:>6} {:>7.1} {:>10.1}M {:>10.1}M {:>10.1}M {:>10.1}M {:>9.2}x",
            language.as_str(),
            corpus.len(),
            bytes as f64 / (1024.0 * 1024.0),
            throughput(bytes, tokenize),
            throughput(bytes, structure),
            throughput(bytes, ts_parse),
            throughput(bytes, ts_walk),
            ts_walk.as_secs_f64() / structure.as_secs_f64(),
        );
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
    }
    if let Some(path) = output
        && let Err(error) = fs::write(&path, report)
    {
        eprintln!("could not write benchmark output {path}: {error}");
        std::process::exit(1);
    }
}

fn take_option(arguments: &mut Vec<String>, name: &str) -> Option<String> {
    let position = arguments.iter().position(|argument| argument == name)?;
    if position + 1 >= arguments.len() {
        eprintln!("{name} requires a value");
        std::process::exit(2);
    }
    arguments.remove(position);
    Some(arguments.remove(position))
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Reproducible correctness gates that do not pretend tree-sitter and this
/// crate expose the same facts.
///
/// The corpus gate proves byte-for-byte losslessness over every selected real
/// source. The small typed fixtures are the oracle for GraphQL, protobuf and
/// the grouped Go declaration repair: every expected fact names its exact
/// source bytes, kind and owner.
fn verify_ground_truth(corpora: &Corpora, languages: &[LanguageGrammar]) {
    let mut files = 0_usize;
    let mut bytes = 0_usize;
    for (language, _) in languages {
        for (path, source) in corpora.get(language).into_iter().flatten() {
            let rebuilt = Tokenizer::new(source, *language)
                .mode(Mode::Lossless)
                .map(|token| token.text(source))
                .collect::<String>();
            assert_eq!(
                rebuilt.as_bytes(),
                source.as_bytes(),
                "{} did not round-trip losslessly: {}",
                language.as_str(),
                path.display()
            );
            files += 1;
            bytes += source.len();
        }
    }
    verify_graphql_contracts();
    verify_protobuf_contracts();
    verify_go_grouped_declarations();
    let graphql_files = corpora.get(&Language::Graphql).map_or(0, Vec::len);
    let protobuf_files = corpora.get(&Language::Protobuf).map_or(0, Vec::len);
    println!(
        "ground_truth=PASS lossless_files={files} lossless_bytes={bytes} \
         graphql_fixture=PASS graphql_corpus_files={graphql_files} \
         protobuf_fixture=PASS protobuf_corpus_files={protobuf_files} \
         go_grouped_declarations=PASS"
    );
}

fn verify_graphql_contracts() {
    let source = concat!(
        "type Query { user(id: ID!): User }\n",
        "fragment Root on Query { user { id } }\n",
        "query Get { ...Root }\n",
    );
    let facts = extract(source, Language::Graphql);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    let field = facts
        .contracts
        .iter()
        .find(|fact| {
            matches!(
                fact.kind,
                ContractKind::GraphqlField {
                    operation: Some(GraphqlOperation::Query),
                    ref return_type,
                } if return_type == "User"
            )
        })
        .expect("GraphQL query field");
    assert_eq!(field.name, "user");
    assert_eq!(field.owner.as_deref(), Some("Query"));
    assert_eq!(&source[field.span.start..field.span.end], "user");
    let spread = facts
        .contracts
        .iter()
        .find(|fact| fact.kind == ContractKind::GraphqlFragmentSpread)
        .expect("GraphQL fragment spread");
    assert_eq!(spread.name, "Root");
    assert_eq!(spread.owner.as_deref(), Some("Get"));
    assert_eq!(&source[spread.span.start..spread.span.end], "Root");
}

fn verify_protobuf_contracts() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "package shop.v1;\n",
        "import public \"common.proto\";\n",
        "message Request {}\n",
        "message Reply {}\n",
        "service Inventory {\n",
        "  rpc Watch(stream Request) returns (stream Reply);\n",
        "}\n",
    );
    let facts = extract(source, Language::Protobuf);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    assert_eq!(facts.imports.len(), 1);
    assert_eq!(facts.imports[0].specifier, "common.proto");
    assert!(facts.imports[0].reexport);
    let rpc = facts
        .contracts
        .iter()
        .find(|fact| {
            matches!(
                fact.kind,
                ContractKind::ProtobufRpc {
                    ref input,
                    ref output,
                    client_streaming: true,
                    server_streaming: true,
                } if input == "Request" && output == "Reply"
            )
        })
        .expect("streaming protobuf RPC");
    assert_eq!(rpc.name, "Watch");
    assert_eq!(rpc.owner.as_deref(), Some("Inventory"));
    assert_eq!(&source[rpc.span.start..rpc.span.end], "Watch");
}

fn verify_go_grouped_declarations() {
    let source = concat!(
        "package main\n",
        "const (\n",
        "  EventAdd = \"added\"\n",
        "  eventDelete = \"deleted\"\n",
        ")\n",
        "var (\n",
        "  endpoint = flag.String(\"endpoint\", \"/events\", \"endpoint\")\n",
        "  topics = []string{EventAdd, eventDelete}\n",
        ")\n",
    );
    let facts = extract(source, Language::Go);
    for (name, kind, line) in [
        ("EventAdd", DeclarationKind::Constant, 3),
        ("eventDelete", DeclarationKind::Constant, 4),
        ("endpoint", DeclarationKind::Variable, 7),
        ("topics", DeclarationKind::Variable, 8),
    ] {
        let fact = facts
            .declarations
            .iter()
            .find(|fact| fact.name == name && fact.kind == kind)
            .unwrap_or_else(|| panic!("missing grouped Go declaration {name}: {facts:?}"));
        assert_eq!(fact.span.line, line);
        assert_eq!(&source[fact.span.start..fact.span.end], name);
    }
}

/// Compares what each side finds, not how fast it finds it.
///
/// Imports are the fact to compare on: every language marks them with a
/// dedicated node type, so tree-sitter's count is a defensible reference
/// rather than an opinion. A shortfall on our side is a real miss; a surplus
/// is either a form tree-sitter splits differently or a false positive, and
/// the per-file worst cases printed underneath say which.
fn audit(corpora: &Corpora, languages: &[LanguageGrammar]) {
    println!(
        "{:<12} {:>6} {:>9} {:>9} {:>8} {:>8} {:>9}",
        "language", "files", "ts", "ours", "missed", "extra", "agreement"
    );
    println!("{}", "-".repeat(68));

    for (language, grammar) in languages {
        if !comparable(*language) {
            continue;
        }
        let Some(corpus) = corpora.get(language) else {
            continue;
        };
        if corpus.is_empty() {
            continue;
        }
        let mut parser = Parser::new();
        if parser.set_language(&grammar()).is_err() {
            continue;
        }
        let (mut theirs, mut ours, mut missed, mut extra) = (0_usize, 0_usize, 0_usize, 0_usize);
        let mut worst: Vec<(i64, String, usize, usize)> = Vec::new();
        for (path, source) in corpus {
            let Some(tree) = parser.parse(source, None) else {
                continue;
            };
            let reference = count_nodes(&tree, *language);
            let mine = extract(source, *language).imports.len();
            theirs += reference;
            ours += mine;
            missed += reference.saturating_sub(mine);
            extra += mine.saturating_sub(reference);
            let gap = i64::try_from(reference).unwrap_or(0) - i64::try_from(mine).unwrap_or(0);
            if gap != 0 {
                worst.push((gap.abs(), path.display().to_string(), reference, mine));
            }
        }
        // A shortfall is a real miss, and the only way to fix one is to see
        // the construct itself rather than a count, so print the import nodes
        // of every file where tree-sitter found more than we did.
        for (path, source) in corpus {
            let Some(tree) = parser.parse(source, None) else {
                continue;
            };
            let reference = count_nodes(&tree, *language);
            let mine = extract(source, *language).imports.len();
            if reference <= mine {
                continue;
            }
            let name = path.display().to_string();
            let name = name.rsplit(['/', '\\']).next().unwrap_or("?").to_owned();
            println!("      MISS {name}: tree-sitter {reference}, ours {mine}");
            for text in import_texts(&tree, *language, source).iter().take(40) {
                println!("           {text}");
            }
        }
        worst.sort_unstable_by_key(|item| std::cmp::Reverse(item.0));
        let agreement = if theirs == 0 {
            100.0
        } else {
            100.0 * (theirs.saturating_sub(missed)) as f64 / theirs as f64
        };
        println!(
            "{:<12} {:>6} {:>9} {:>9} {:>8} {:>8} {:>8.1}%",
            language.as_str(),
            corpus.len(),
            theirs,
            ours,
            missed,
            extra,
            agreement
        );
        for (_, path, reference, mine) in worst.iter().take(3) {
            let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
            println!("             worst: {name} (tree-sitter {reference}, ours {mine})");
        }
    }
}

/// Whether this language has a node type the two sides can be compared on.
/// C and C++ preprocessor includes and SQL table references have no single
/// equivalent node, so they are left out rather than compared unfairly.
const fn comparable(language: Language) -> bool {
    matches!(
        language,
        Language::JavaScript
            | Language::TypeScript
            | Language::Python
            | Language::Rust
            | Language::Go
            | Language::Java
            | Language::CSharp
    )
}

/// Whether a tree-sitter node is a module dependency.
///
/// Two of these are narrower or wider than the node type alone. A Rust
/// `mod_item` with a body defines the module in this file rather than pulling
/// in another, so only the bodyless form counts. A JavaScript
/// `export_statement` is a dependency exactly when it carries a source, which
/// is what `export ... from` does.
fn is_import(language: Language, node: &tree_sitter::Node) -> Option<bool> {
    Some(match language {
        Language::JavaScript | Language::TypeScript => match node.kind() {
            "import_statement" => true,
            "export_statement" => node.child_by_field_name("source").is_some(),
            _ => false,
        },
        Language::Python => matches!(node.kind(), "import_statement" | "import_from_statement"),
        Language::Rust => match node.kind() {
            "use_declaration" => true,
            "mod_item" => node.child_by_field_name("body").is_none(),
            _ => false,
        },
        Language::Go => node.kind() == "import_spec",
        Language::Java => node.kind() == "import_declaration",
        Language::CSharp => node.kind() == "using_directive",
        _ => return None,
    })
}

/// The source text of every import node, one line each, for reading by eye.
fn import_texts(tree: &tree_sitter::Tree, language: Language, source: &str) -> Vec<String> {
    let mut cursor = tree.walk();
    let mut found = Vec::new();
    let mut descend = true;
    loop {
        if descend {
            let node = cursor.node();
            if is_import(language, &node) == Some(true) {
                let text = source[node.byte_range()]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                found.push(text.chars().take(110).collect::<String>());
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        if cursor.goto_next_sibling() {
            descend = true;
            continue;
        }
        if !cursor.goto_parent() {
            return found;
        }
        descend = false;
    }
}

fn count_nodes(tree: &tree_sitter::Tree, language: Language) -> usize {
    let mut cursor = tree.walk();
    let mut count = 0_usize;
    let mut descend = true;
    loop {
        if descend {
            if is_import(language, &cursor.node()) == Some(true) {
                count += 1;
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        if cursor.goto_next_sibling() {
            descend = true;
            continue;
        }
        if !cursor.goto_parent() {
            return count;
        }
        descend = false;
    }
}

/// Counts every node, which is the cheapest honest stand-in for the traversal
/// a consumer of a tree-sitter parse still has to perform to get facts out.
fn walk(tree: &tree_sitter::Tree) -> usize {
    let mut cursor = tree.walk();
    let mut count = 0_usize;
    let mut descend = true;
    loop {
        if descend && cursor.goto_first_child() {
            count += 1;
            continue;
        }
        if cursor.goto_next_sibling() {
            count += 1;
            descend = true;
            continue;
        }
        if !cursor.goto_parent() {
            return count;
        }
        descend = false;
    }
}

fn throughput(bytes: usize, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        return 0.0;
    }
    bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
}

/// Times one pass over the corpus. The caller keeps the fastest of several,
/// because contention can only ever make a run slower than the work costs.
fn time<T>(run: impl FnOnce() -> T) -> Duration {
    let started = Instant::now();
    let outcome = run();
    let elapsed = started.elapsed();
    std::hint::black_box(outcome);
    elapsed
}

/// One corpus file: where it came from, and what it contains.
type Entry = (std::path::PathBuf, String);

type Corpora = HashMap<Language, Vec<Entry>>;

/// Reads every selected language in one deterministic filesystem traversal.
fn collect_all(roots: &[String], languages: &[LanguageGrammar]) -> Corpora {
    let allowed = languages
        .iter()
        .map(|(language, _)| *language)
        .collect::<HashSet<_>>();
    let mut corpora = HashMap::new();
    let mut totals = HashMap::new();
    for root in roots {
        visit(Path::new(root), &allowed, &mut corpora, &mut totals);
    }
    corpora
}

fn visit(
    path: &Path,
    allowed: &HashSet<Language>,
    corpora: &mut Corpora,
    totals: &mut HashMap<Language, usize>,
) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if skip_directory(&name) {
            continue;
        }
        if path.is_dir() {
            visit(&path, allowed, corpora, totals);
            continue;
        }
        let Some(language) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(Language::from_extension)
        else {
            continue;
        };
        if !allowed.contains(&language)
            || totals.get(&language).copied().unwrap_or(0) >= CORPUS_LIMIT
        {
            continue;
        }
        if let Ok(source) = fs::read_to_string(&path) {
            *totals.entry(language).or_default() += source.len();
            corpora.entry(language).or_default().push((path, source));
        }
    }
}

fn skip_directory(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "coverage"
                | "out"
                | "__pycache__"
                | "venv"
        )
}
