//! Throughput comparison against tree-sitter on the same source files.
//!
//! Two things are measured because they are two different jobs. `tokenize`
//! against `tree-sitter parse` compares the cost of turning bytes into a
//! traversable form. `extract` against `parse + walk` compares the cost of
//! answering the question a consumer actually asks: what does this file
//! declare, import and call. Reporting only the first would flatter whichever
//! side does less work.
//!
//! Usage: `competitor-bench <corpus-dir>...`

use std::env;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use tree_sitter::{Language as TsLanguage, Parser};
use weavatrix_parse::{Language, Mode, Tokenizer, extract};

/// How many times each measurement is repeated; the median is reported.
const ROUNDS: usize = 7;

/// Largest corpus per language, so one huge vendored tree cannot dominate.
const CORPUS_LIMIT: usize = 8 * 1024 * 1024;

fn main() {
    let mut roots = env::args().skip(1).collect::<Vec<_>>();
    let auditing = roots.iter().any(|argument| argument == "--audit");
    roots.retain(|argument| argument != "--audit");
    if roots.is_empty() {
        eprintln!("usage: competitor-bench [--audit] <corpus-dir>...");
        std::process::exit(2);
    }

    let languages: &[(Language, fn() -> TsLanguage)] = &[
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
        // SQL has no first-party tree-sitter grammar; this is the maintained
        // community one, which is what a consumer would actually reach for.
        (Language::Sql, || tree_sitter_sequel::LANGUAGE.into()),
    ];

    if auditing {
        audit(&roots, languages);
        return;
    }

    println!(
        "{:<12} {:>6} {:>7} {:>11} {:>11} {:>11} {:>11} {:>10}",
        "language", "files", "MB", "tokenize", "extract", "ts parse", "ts walk", "extract/ts"
    );
    println!("{}", "-".repeat(88));

    for (language, grammar) in languages {
        let corpus = collect(&roots, *language);
        if corpus.is_empty() {
            continue;
        }
        let bytes = corpus.iter().map(|(_, source)| source.len()).sum::<usize>();

        let mut parser = Parser::new();
        if parser.set_language(&grammar()).is_err() {
            eprintln!("{}: grammar rejected by this tree-sitter", language.as_str());
            continue;
        }

        // The four measurements are interleaved within each round rather than
        // run one after another. On a machine doing anything else, load drifts
        // over seconds, and consecutive blocks would charge that drift to
        // whichever implementation happened to run during it - which is how a
        // first run produced a `walk` faster than the `parse` it contains.
        let mut tokenize = Duration::MAX;
        let mut structure = Duration::MAX;
        let mut ts_parse = Duration::MAX;
        let mut ts_walk = Duration::MAX;
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
                for (_, source) in &corpus {
                    if let Some(tree) = parser.parse(source, None) {
                        count += tree.root_node().child_count();
                    }
                }
                count
            });
            let four = time(|| {
                let mut count = 0_usize;
                for (_, source) in &corpus {
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
            tokenize = tokenize.min(one);
            structure = structure.min(two);
            ts_parse = ts_parse.min(three);
            ts_walk = ts_walk.min(four);
        }

        println!(
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
    }
}

/// Compares what each side finds, not how fast it finds it.
///
/// Imports are the fact to compare on: every language marks them with a
/// dedicated node type, so tree-sitter's count is a defensible reference
/// rather than an opinion. A shortfall on our side is a real miss; a surplus
/// is either a form tree-sitter splits differently or a false positive, and
/// the per-file worst cases printed underneath say which.
fn audit(roots: &[String], languages: &[(Language, fn() -> TsLanguage)]) {
    println!(
        "{:<12} {:>6} {:>9} {:>9} {:>8} {:>8} {:>9}",
        "language", "files", "ts", "ours", "missed", "extra", "agreement"
    );
    println!("{}", "-".repeat(68));

    for (language, grammar) in languages {
        if !comparable(*language) {
            continue;
        }
        let corpus = collect(roots, *language);
        if corpus.is_empty() {
            continue;
        }
        let mut parser = Parser::new();
        if parser.set_language(&grammar()).is_err() {
            continue;
        }
        let (mut theirs, mut ours, mut missed, mut extra) = (0_usize, 0_usize, 0_usize, 0_usize);
        let mut worst: Vec<(i64, String, usize, usize)> = Vec::new();
        for (path, source) in &corpus {
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
        for (path, source) in &corpus {
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
        worst.sort_unstable_by(|left, right| right.0.cmp(&left.0));
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

/// Reads every file of one language under the given roots, up to the cap.
fn collect(roots: &[String], language: Language) -> Vec<Entry> {
    let mut corpus = Vec::new();
    let mut total = 0_usize;
    for root in roots {
        visit(Path::new(root), language, &mut corpus, &mut total);
    }
    corpus
}

fn visit(path: &Path, language: Language, corpus: &mut Vec<Entry>, total: &mut usize) {
    if *total >= CORPUS_LIMIT {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            visit(&path, language, corpus, total);
            continue;
        }
        let matches = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(Language::from_extension)
            == Some(language);
        if !matches {
            continue;
        }
        if let Ok(source) = fs::read_to_string(&path) {
            *total += source.len();
            corpus.push((path, source));
        }
        if *total >= CORPUS_LIMIT {
            return;
        }
    }
}


