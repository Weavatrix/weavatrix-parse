//! Stable-API facts and throughput snapshot used to compare parser revisions.
//!
//! Keep this tool limited to API fields that existed in the baseline parser
//! commit. The same source is compiled twice by `compare-parser-revisions.mjs`:
//! once against the dirty/current checkout and once against an isolated Git
//! worktree. That makes the comparison independent of benchmark-code changes.

use std::env;
use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use weavatrix_parse::{Language, extract};

mod corpus;

use corpus::{Entry, collect};

const CORPUS_LIMIT: usize = 8 * 1024 * 1024;
const ROUNDS: usize = 5;

const LANGUAGES: &[Language] = &[
    Language::JavaScript,
    Language::TypeScript,
    Language::Python,
    Language::Rust,
    Language::Go,
    Language::Java,
    Language::CSharp,
    Language::C,
    Language::Cpp,
    Language::Sql,
    Language::Swift,
    Language::Terraform,
    Language::Xml,
    Language::Markdown,
    Language::Bash,
];

fn main() {
    let mut summary_only = false;
    let roots = env::args()
        .skip(1)
        .filter_map(|argument| {
            if argument == "--summary-only" {
                summary_only = true;
                None
            } else {
                Some(PathBuf::from(argument))
            }
        })
        .collect::<Vec<_>>();
    if roots.is_empty() {
        eprintln!("usage: weavatrix-parse-regression-bench <corpus-dir>...");
        std::process::exit(2);
    }
    println!("schema=weavatrix.parse-regression.v1 rounds={ROUNDS} cap={CORPUS_LIMIT}");
    let corpora = collect(&roots, LANGUAGES, CORPUS_LIMIT);
    for language in LANGUAGES {
        let Some(corpus) = corpora.get(language) else {
            continue;
        };
        if corpus.is_empty() {
            continue;
        }
        let mut language_hash = StableHash::new();
        let mut declarations = 0_usize;
        let mut imports = 0_usize;
        let mut references = 0_usize;
        let bytes = corpus.iter().map(|entry| entry.source.len()).sum::<usize>();
        for entry in corpus {
            let facts = extract(&entry.source, *language);
            declarations += facts.declarations.len();
            imports += facts.imports.len();
            references += facts.references.len();
            let mut hash = StableHash::new();
            hash.write(entry.identity.as_bytes());
            write_stable_declarations(&mut hash, &facts.declarations);
            hash.write(format!("{:?}", facts.imports).as_bytes());
            hash.write(format!("{:?}", facts.references).as_bytes());
            language_hash.write(entry.identity.as_bytes());
            language_hash.write_u64(hash.finish());
            if !summary_only {
                println!(
                    "F\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}",
                    language.as_str(),
                    escape(&entry.identity),
                    entry.source.len(),
                    facts.declarations.len(),
                    facts.imports.len(),
                    facts.references.len(),
                    hash.finish()
                );
                print_exact_facts(*language, entry, &facts);
            }
        }

        // Warm once, then retain the fastest complete extraction. External
        // orchestration alternates current/baseline processes and takes the
        // median across processes, so load cannot systematically favour one.
        let _ = extract_all(corpus, *language);
        let mut elapsed = Duration::MAX;
        for _ in 0..ROUNDS {
            let started = Instant::now();
            let count = extract_all(corpus, *language);
            let sample = started.elapsed();
            black_box(count);
            elapsed = elapsed.min(sample);
        }
        println!(
            "L\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{}",
            language.as_str(),
            corpus.len(),
            bytes,
            declarations,
            imports,
            references,
            language_hash.finish(),
            elapsed.as_nanos()
        );
    }
}

fn print_exact_facts(language: Language, entry: &Entry, facts: &weavatrix_parse::Facts) {
    for declaration in &facts.declarations {
        println!(
            "D\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}",
            language.as_str(),
            escape(&entry.identity),
            escape(&declaration.name),
            declaration.kind,
            declaration.span.start,
            declaration.span.end,
            declaration.span.line,
            escape(declaration.owner.as_deref().unwrap_or(""))
        );
    }
    for import in &facts.imports {
        println!(
            "I\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            language.as_str(),
            escape(&entry.identity),
            escape(&import.specifier),
            import.reexport,
            import.span.start,
            import.span.end,
            import.span.line
        );
    }
    for reference in &facts.references {
        println!(
            "R\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}",
            language.as_str(),
            escape(&entry.identity),
            escape(&reference.name),
            reference.kind,
            reference.span.start,
            reference.span.end,
            reference.span.line,
            escape(reference.owner.as_deref().unwrap_or(""))
        );
    }
}

/// Hashes only the declaration fields that existed in the baseline API.
///
/// The current benchmark harness is compiled against both parser revisions.
/// New additive metadata such as `Declaration::test_only` must not make every
/// unchanged declaration look like a semantic regression merely because the
/// derived `Debug` representation gained a field.
fn write_stable_declarations(hash: &mut StableHash, declarations: &[weavatrix_parse::Declaration]) {
    for declaration in declarations {
        hash.write(declaration.name.as_bytes());
        hash.write(b"\0");
        hash.write(format!("{:?}", declaration.kind).as_bytes());
        hash.write(b"\0");
        hash.write(declaration.span.start.to_string().as_bytes());
        hash.write(b"\0");
        hash.write(declaration.span.end.to_string().as_bytes());
        hash.write(b"\0");
        hash.write(declaration.span.line.to_string().as_bytes());
        hash.write(b"\0");
        hash.write(declaration.span.column.to_string().as_bytes());
        hash.write(b"\0");
        hash.write(declaration.owner.as_deref().unwrap_or("").as_bytes());
        hash.write(b"\0");
        hash.write(if declaration.exported { b"1" } else { b"0" });
        hash.write(b"\n");
    }
}

fn extract_all(corpus: &[Entry], language: Language) -> usize {
    corpus
        .iter()
        .map(|entry| {
            let facts = extract(&entry.source, language);
            facts.declarations.len() + facts.imports.len() + facts.references.len()
        })
        .sum()
}

fn escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            '\n' => output.push_str("\\n"),
            other => {
                let _ = output.write_char(other);
            }
        }
    }
    output
}

struct StableHash(u64);

impl StableHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Field separator: concatenated inputs must not alias split inputs.
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    const fn finish(&self) -> u64 {
        self.0
    }
}
