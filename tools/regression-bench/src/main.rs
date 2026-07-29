//! Stable-API facts and throughput snapshot used to compare parser revisions.
//!
//! Keep this tool limited to API fields that existed in the baseline parser
//! commit. The same source is compiled twice by `compare-parser-revisions.mjs`:
//! once against the dirty/current checkout and once against an isolated Git
//! worktree. That makes the comparison independent of benchmark-code changes.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use weavatrix_parse::{Language, extract};

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
    let roots = env::args().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if roots.is_empty() {
        eprintln!("usage: weavatrix-parse-regression-bench <corpus-dir>...");
        std::process::exit(2);
    }
    println!("schema=weavatrix.parse-regression.v1 rounds={ROUNDS} cap={CORPUS_LIMIT}");
    let corpora = collect(&roots);
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
            hash.write(format!("{:?}", facts.declarations).as_bytes());
            hash.write(format!("{:?}", facts.imports).as_bytes());
            hash.write(format!("{:?}", facts.references).as_bytes());
            language_hash.write(entry.identity.as_bytes());
            language_hash.write_u64(hash.finish());
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

fn extract_all(corpus: &[Entry], language: Language) -> usize {
    corpus
        .iter()
        .map(|entry| {
            let facts = extract(&entry.source, language);
            facts.declarations.len() + facts.imports.len() + facts.references.len()
        })
        .sum()
}

struct Entry {
    identity: String,
    source: String,
}

fn collect(roots: &[PathBuf]) -> HashMap<Language, Vec<Entry>> {
    let allowed = LANGUAGES.iter().copied().collect::<HashSet<_>>();
    let mut corpora = HashMap::new();
    let mut totals = HashMap::new();
    for (root_index, root) in roots.iter().enumerate() {
        visit(root, root, root_index, &allowed, &mut corpora, &mut totals);
    }
    corpora
}

fn visit(
    root: &Path,
    path: &Path,
    root_index: usize,
    allowed: &HashSet<Language>,
    corpora: &mut HashMap<Language, Vec<Entry>>,
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
            visit(root, &path, root_index, allowed, corpora, totals);
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
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let identity = format!(
                "{root_index}/{}",
                relative.to_string_lossy().replace('\\', "/")
            );
            *totals.entry(language).or_default() += source.len();
            corpora
                .entry(language)
                .or_default()
                .push(Entry { identity, source });
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
