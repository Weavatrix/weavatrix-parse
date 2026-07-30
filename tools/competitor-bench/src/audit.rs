use tree_sitter::Parser;
use weavatrix_parse::{Language, extract};

use crate::config::LanguageGrammar;
use crate::corpus::{Corpora, Entry};

/// Compares what each side finds, not how fast it finds it.
///
/// Imports are the fact to compare on: every language marks them with a
/// dedicated node type, so tree-sitter's count is a defensible reference
/// rather than an opinion. A shortfall on our side is a real miss; a surplus
/// is either a form tree-sitter splits differently or a false positive, and
/// the per-file worst cases printed underneath say which.
pub(crate) fn run(corpora: &Corpora, languages: &[LanguageGrammar]) {
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
        let summary = compare_imports(&mut parser, corpus, *language);
        print_misses(&mut parser, corpus, *language);
        print_summary(*language, corpus.len(), summary);
    }
}

type ImportSummary = (usize, usize, usize, usize, Vec<(i64, String, usize, usize)>);

fn compare_imports(parser: &mut Parser, corpus: &[Entry], language: Language) -> ImportSummary {
    let (mut theirs, mut ours, mut missed, mut extra) = (0_usize, 0_usize, 0_usize, 0_usize);
    let mut worst = Vec::new();
    for (path, source) in corpus {
        let Some(tree) = parser.parse(source, None) else {
            continue;
        };
        let reference = count_nodes(&tree, language);
        let mine = extract(source, language).imports.len();
        theirs += reference;
        ours += mine;
        missed += reference.saturating_sub(mine);
        extra += mine.saturating_sub(reference);
        let gap = i64::try_from(reference).unwrap_or(0) - i64::try_from(mine).unwrap_or(0);
        if gap != 0 {
            worst.push((gap.abs(), path.display().to_string(), reference, mine));
        }
    }
    (theirs, ours, missed, extra, worst)
}

fn print_misses(parser: &mut Parser, corpus: &[Entry], language: Language) {
    // A shortfall is a real miss, and the only way to fix one is to see the
    // construct itself rather than a count, so print the import nodes of every
    // file where tree-sitter found more than we did.
    for (path, source) in corpus {
        let Some(tree) = parser.parse(source, None) else {
            continue;
        };
        let reference = count_nodes(&tree, language);
        let mine = extract(source, language).imports.len();
        if reference <= mine {
            continue;
        }
        let name = path.display().to_string();
        let name = name.rsplit(['/', '\\']).next().unwrap_or("?").to_owned();
        println!("      MISS {name}: tree-sitter {reference}, ours {mine}");
        for text in import_texts(&tree, language, source).iter().take(40) {
            println!("           {text}");
        }
    }
}

fn print_summary(language: Language, files: usize, summary: ImportSummary) {
    let (theirs, ours, missed, extra, mut worst) = summary;
    worst.sort_unstable_by_key(|item| std::cmp::Reverse(item.0));
    let agreement = if theirs == 0 {
        100.0
    } else {
        100.0 * (theirs.saturating_sub(missed)) as f64 / theirs as f64
    };
    println!(
        "{:<12} {:>6} {:>9} {:>9} {:>8} {:>8} {:>8.1}%",
        language.as_str(),
        files,
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
