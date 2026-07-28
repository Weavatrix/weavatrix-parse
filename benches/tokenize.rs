//! Throughput of the tokenizer, in both modes, over generated sources.
//!
//! Competitor numbers live in `tools/competitor-bench`, which is a separate
//! workspace: comparing against tree-sitter means compiling C grammars, and
//! that dependency must not reach the published crate. This bench measures
//! only what this crate ships.

use std::hint::black_box;
use std::time::Instant;
use weavatrix_parse::{Language, Mode, Tokenizer};

fn main() {
    println!("statistic=median runs=11 warmups=2");
    for (language, source) in [
        (Language::TypeScript, typescript_source()),
        (Language::Python, python_source()),
        (Language::Rust, rust_source()),
    ] {
        for mode in [Mode::Lossless, Mode::Lite] {
            let measure = || {
                let count = Tokenizer::new(&source, language)
                    .mode(mode)
                    .filter(|token| !token.is_trivia())
                    .count();
                black_box(count);
            };
            let median = median_ms(measure);
            let megabytes = source.len() as f64 / (1024.0 * 1024.0);
            println!(
                "language={} mode={mode:?} bytes={} median_ms={median:.3} mb_per_s={:.1}",
                language.as_str(),
                source.len(),
                megabytes / (median / 1000.0)
            );
        }
    }
}

fn median_ms(mut operation: impl FnMut()) -> f64 {
    for _ in 0..2 {
        operation();
    }
    let mut samples = Vec::new();
    for _ in 0..11 {
        let started = Instant::now();
        operation();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn repeat(unit: &str, times: usize) -> String {
    let mut source = String::with_capacity(unit.len() * times);
    for index in 0..times {
        source.push_str(&unit.replace("$N", &index.to_string()));
    }
    source
}

fn typescript_source() -> String {
    repeat(
        "import type { Shape$N } from './shapes/$N';\n\
         import { helper$N } from '@app/helpers';\n\
         // a comment mentioning app.get('/not-a-route')\n\
         export class Service$N {\n\
         \x20 private readonly cache = new Map<string, Shape$N>();\n\
         \x20 async run(input: string): Promise<number> {\n\
         \x20   const pattern = /ab\\/c[/]+/g;\n\
         \x20   const label = `item ${input} of $N`;\n\
         \x20   return helper$N(label, pattern) / 2;\n\
         \x20 }\n\
         }\n",
        2_000,
    )
}

fn python_source() -> String {
    repeat(
        "from pkg.module$N import helper$N\n\n\
         class Service$N(Base):\n\
         \x20   \"\"\"Docstring with # not a comment and 'quotes'.\"\"\"\n\
         \x20   def run(self, value):\n\
         \x20       total = value / 2  # real comment\n\
         \x20       return helper$N(total)\n\n",
        2_000,
    )
}

fn rust_source() -> String {
    repeat(
        "use crate::module$N::Helper$N;\n\
         /* outer /* nested */ still open */\n\
         pub struct Service$N {\n\
         \x20   cache: Vec<Helper$N>,\n\
         }\n\
         impl Service$N {\n\
         \x20   pub fn run(&self, value: u32) -> u32 {\n\
         \x20       let raw = r#\"a \"quoted\" path\"#;\n\
         \x20       black_box(raw);\n\
         \x20       value / 2\n\
         \x20   }\n\
         }\n",
        2_000,
    )
}
