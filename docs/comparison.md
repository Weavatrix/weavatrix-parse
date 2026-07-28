# weavatrix-parse against tree-sitter

Measured with `tools/competitor-bench`, a workspace kept outside the published
crate so the tree-sitter C grammars never reach it. Corpus: every file of each
language under `C:\Users\SergiiZiborov\Documents\GitHub`, capped at 8 MB per
language. tree-sitter 0.26.11 with the first-party grammars, except SQL, which
has no first-party grammar and uses the maintained `tree-sitter-sequel`.

## Throughput

Four measurements are interleaved inside every round rather than run in
separate blocks, and the fastest of eight rounds is kept. An earlier version
ran each measurement as its own block and produced a `walk` faster than the
`parse` it contains — a physical impossibility, and proof that consecutive
blocks were charging machine load to whichever implementation happened to be
running. Absolute throughput still moves with load; the ratio does not, because
both sides now meet the same load in the same round.

`tokenize` is our Lite token stream. `extract` is tokens plus structural facts —
declarations, imports, calls with spans. `ts parse` is tree-sitter building a
tree. `ts walk` is that plus one full traversal, which is the cheapest way a
consumer can actually reach the facts, and so the fair comparison for `extract`.

| language   | files | MB  | tokenize | extract | ts parse | ts walk | extract/ts walk |
|------------|-------|-----|----------|---------|----------|---------|-----------------|
| javascript | 1047  | 8.1 | 72.0 MB/s | 40.3 MB/s | 5.0 MB/s | 4.0 MB/s | 10.1× |
| typescript | 592   | 8.1 | 73.6 | 41.9 | 5.4 | 4.4 | 9.6× |
| python     | 1066  | 8.1 | 173.1 | 131.5 | 9.2 | 8.7 | 15.1× |
| rust       | 1117  | 7.4 | 41.9 | 22.3 | 2.7 | 2.1 | 10.8× |
| go         | 880   | 4.2 | 81.6 | 45.7 | 5.0 | 4.2 | 11.0× |
| java       | 399   | 2.4 | 53.2 | 24.7 | 3.4 | 3.3 | 7.6× |
| csharp     | 7     | 0.1 | 87.5 | 45.9 | 1.7 | 1.7 | 26.6× |
| sql        | 20    | 0.1 | 77.0 | 46.5 | 1.1 | 1.2 | 37.4× |

The C# and SQL corpora are too small to draw a conclusion from; they are listed
so the gap is visible rather than hidden. The six languages with real corpora
land between 7.6× and 15.1×.

## Agreement

Speed is worth nothing if the facts are wrong, so the same corpus is compared on
what each side finds. Imports are the fact to compare on, because every grammar
marks them with a dedicated node type, which makes tree-sitter a reference
rather than an opinion.

Two of the reference definitions needed narrowing before the comparison meant
anything. A Rust `mod_item` covers both `mod x;` and `mod x { ... }`, but only
the first pulls in another file, so only the bodyless form counts. A JavaScript
`export_statement` is a dependency exactly when it carries a source, which is
what `export ... from` does.

| language   | files | tree-sitter | ours | missed | extra | agreement |
|------------|-------|-------------|------|--------|-------|-----------|
| javascript | 1047  | 1471 | 3832 | 2 | 2363 | 99.9% |
| typescript | 592   | 2373 | 2627 | 7 | 261 | 99.7% |
| python     | 1064  | 5814 | 5822 | 0 | 8 | 100.0% |
| rust       | 1118  | 5654 | 5657 | 0 | 3 | 100.0% |
| go         | 880   | 5080 | 5080 | 0 | 0 | 100.0% |
| java       | 399   | 4586 | 4586 | 0 | 0 | 100.0% |
| csharp     | 7     | 28 | 28 | 0 | 0 | 100.0% |

The JavaScript and TypeScript surpluses were read rather than assumed. In
JavaScript they are `require()` calls: `model.js` has 71 of them and no ESM
import at all, and CommonJS is a real dependency that `import_statement` cannot
see. In TypeScript they are `typeof import("...")` type positions, which
`@types/node/process.d.ts` uses about a hundred times, and which are also real
dependencies. Both are cases of being more complete than a single node type, not
of being wrong.

Rust reached 100% only after two defects the comparison exposed:

A character literal holding a quote — `head.contains(['.', '"', '+'])` — left the
tokenizer treating the double quote as opening a string, which then ran on for
hundreds of lines and swallowed every declaration and import after it. Treating
`'` as ordinary punctuation is what kept lifetimes working and what caused this.
The closing quote separates the two forms, so that is what now decides.
Agreement went from 97.2% to 99.8%.

`pub(crate) use x;` was invisible because the import path stepped over modifiers
but not over a parenthesised visibility scope, which the declaration path
already did. Fixing that closed the last 9 misses.

## What tree-sitter does that this crate does not

This matters more than the table above, and none of it is close.

A full concrete syntax tree with error recovery. tree-sitter yields a typed tree
with named fields and keeps parsing through syntax errors, which is why editors
use it. This crate yields a flat token stream and a fixed set of facts; there is
no expression structure and no operator precedence.

Incremental reparsing. Editing one line and reparsing costs tree-sitter a
fraction of a full parse. Here every parse is a full parse — acceptable when the
unit of work is a repository scan, useless for an editor.

A query language. tree-sitter's S-expression patterns let a consumer ask for
shapes the parser author never anticipated. Getting a new fact out of this crate
means writing Rust.

Breadth. Roughly two hundred grammars exist against the eleven languages here.

Whether a grammar is right. A generated grammar is checked against the language
specification. These extractors are checked against tests and against the
agreement measurement above, which is weaker evidence.

## What this crate does that tree-sitter does not

No dependencies, no C, no build script, `unsafe` forbidden — which is the entire
reason it exists, given what a C grammar with a build step does to a supply-chain
score. One crate covers eleven languages instead of one crate per grammar plus a
runtime. And it is between seven and fifteen times faster at producing facts,
because it never builds the tree it would then have to walk.

## The gap that matters for a compiler

Using Weavatrix inside a compiler, or to pick which parts of a JavaScript
codebase can move to WebAssembly, or to translate Go to Rust, needs expression
structure this crate deliberately does not produce. Facts — declarations,
imports, calls — are enough to build a dependency graph and nowhere near enough
to reason about what a function computes.

That is a third mode on the same tokenizer rather than a different crate:
`Mode::Lossless` already guarantees round-trip, which is the hard part of a
faithful tree. It has not been built and should not be claimed until it is.

## Adding a language

Lexical rules are data, and the brace-scoped languages share one walk driven by
keyword tables. Solidity was added for the cost of two tables and a test, which
is the honest measure of what the next language costs — provided it is
brace-scoped. A language with its own scoping model, as Python and SQL have,
costs a module.
