use std::time::{Duration, Instant};

use tree_sitter::Parser;
use weavatrix_parse::{Language, Mode, Tokenizer, extract};

use crate::config::LanguageGrammar;
use crate::corpus::{CORPUS_LIMIT, Corpora, Entry};

/// How many measured rounds follow the warm-up; the median is reported.
const ROUNDS: usize = 7;

pub(crate) fn run(corpora: &Corpora, languages: &[LanguageGrammar]) -> String {
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
        if let Some(line) = benchmark_language(*language, *grammar, corpus) {
            println!("{line}");
            report.push_str(&line);
            report.push('\n');
        }
    }
    report
}

fn benchmark_language(
    language: Language,
    grammar: fn() -> tree_sitter::Language,
    corpus: &[Entry],
) -> Option<String> {
    if corpus.is_empty() {
        return None;
    }
    let bytes = corpus.iter().map(|(_, source)| source.len()).sum::<usize>();
    let mut parser = Parser::new();
    if parser.set_language(&grammar()).is_err() {
        eprintln!(
            "{}: grammar rejected by this tree-sitter",
            language.as_str()
        );
        return None;
    }
    let mut samples = measure(&mut parser, language, corpus);
    Some(format_line(
        language,
        corpus.len(),
        bytes,
        samples.medians(),
    ))
}

struct Samples {
    tokenize: Vec<Duration>,
    structure: Vec<Duration>,
    ts_parse: Vec<Duration>,
    ts_walk: Vec<Duration>,
}

impl Samples {
    fn new() -> Self {
        Self {
            tokenize: Vec::with_capacity(ROUNDS),
            structure: Vec::with_capacity(ROUNDS),
            ts_parse: Vec::with_capacity(ROUNDS),
            ts_walk: Vec::with_capacity(ROUNDS),
        }
    }

    fn medians(&mut self) -> Measurements {
        Measurements {
            tokenize: median(&mut self.tokenize),
            structure: median(&mut self.structure),
            ts_parse: median(&mut self.ts_parse),
            ts_walk: median(&mut self.ts_walk),
        }
    }
}

struct Measurements {
    tokenize: Duration,
    structure: Duration,
    ts_parse: Duration,
    ts_walk: Duration,
}

fn measure(parser: &mut Parser, language: Language, corpus: &[Entry]) -> Samples {
    let mut samples = Samples::new();
    // The four measurements are interleaved within each round. Consecutive
    // blocks would charge machine-load drift to whichever implementation
    // happened to run during it.
    for round in 0..=ROUNDS {
        let tokenize = time(|| {
            corpus
                .iter()
                .map(|(_, source)| Tokenizer::new(source, language).mode(Mode::Lite).count())
                .sum::<usize>()
        });
        let structure = time(|| {
            corpus
                .iter()
                .map(|(_, source)| {
                    let facts = extract(source, language);
                    facts.declarations.len() + facts.imports.len() + facts.references.len()
                })
                .sum::<usize>()
        });
        let ts_parse = time(|| parse_roots(parser, corpus));
        let ts_walk = time(|| parse_and_walk(parser, corpus));
        // Round zero only warms caches and allocators.
        if round > 0 {
            samples.tokenize.push(tokenize);
            samples.structure.push(structure);
            samples.ts_parse.push(ts_parse);
            samples.ts_walk.push(ts_walk);
        }
    }
    samples
}

fn parse_roots(parser: &mut Parser, corpus: &[Entry]) -> usize {
    let mut count = 0_usize;
    for (_, source) in corpus {
        if let Some(tree) = parser.parse(source, None) {
            count += tree.root_node().child_count();
        }
    }
    count
}

fn parse_and_walk(parser: &mut Parser, corpus: &[Entry]) -> usize {
    let mut count = 0_usize;
    for (_, source) in corpus {
        if let Some(tree) = parser.parse(source, None) {
            count += walk(&tree);
        }
    }
    count
}

fn format_line(
    language: Language,
    files: usize,
    bytes: usize,
    measurements: Measurements,
) -> String {
    format!(
        "{:<12} {:>6} {:>7.1} {:>10.1}M {:>10.1}M {:>10.1}M {:>10.1}M {:>9.2}x",
        language.as_str(),
        files,
        bytes as f64 / (1024.0 * 1024.0),
        throughput(bytes, measurements.tokenize),
        throughput(bytes, measurements.structure),
        throughput(bytes, measurements.ts_parse),
        throughput(bytes, measurements.ts_walk),
        measurements.ts_walk.as_secs_f64() / measurements.structure.as_secs_f64(),
    )
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
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

/// Times one pass over the corpus.
fn time<T>(run: impl FnOnce() -> T) -> Duration {
    let started = Instant::now();
    let outcome = run();
    let elapsed = started.elapsed();
    std::hint::black_box(outcome);
    elapsed
}
