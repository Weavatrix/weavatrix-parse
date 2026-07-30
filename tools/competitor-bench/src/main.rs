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

mod audit;
mod benchmark;
mod config;
mod contracts;
mod corpus;

use std::fs;

use config::{Config, LANGUAGE_GRAMMARS};

fn main() {
    let config = Config::from_env();
    let corpora = corpus::collect_all(&config.roots, LANGUAGE_GRAMMARS);
    if config.verifying {
        contracts::verify_ground_truth(&corpora, LANGUAGE_GRAMMARS);
        return;
    }
    if config.auditing {
        audit::run(&corpora, LANGUAGE_GRAMMARS);
        return;
    }

    let report = benchmark::run(&corpora, LANGUAGE_GRAMMARS);
    if let Some(path) = config.output
        && let Err(error) = fs::write(&path, report)
    {
        eprintln!("could not write benchmark output {path}: {error}");
        std::process::exit(1);
    }
}
