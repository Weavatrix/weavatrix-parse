# weavatrix-parse

Source tokenizer and structural extractor for repository intelligence, and the
parsing layer the Weavatrix engine is built on. No dependencies, no generated
grammars, no C toolchain, `unsafe` forbidden.

## Why this exists

Line-oriented scanning is wrong in ways that change answers. A route written
inside a comment becomes an endpoint. A `//` inside a string ends the line
early. A declaration spanning three lines disappears. A class body yields no
methods. Every one of those is a tokenizer problem, so this crate starts with a
tokenizer instead of pattern-matching lines.

The alternative was to depend on tree-sitter, which means a C grammar and a
build step per language. For a tool whose whole argument is supply-chain
clarity, that is the wrong trade — so the tokenizer is owned outright, and the
cost of owning it is measured against tree-sitter rather than asserted.

## Lossless by default

Every byte of the input belongs to exactly one token, whitespace and comments
included, so concatenating the token texts reproduces the source exactly. The
test suite asserts that invariant. It is what lets the same stream serve a
formatter, a source-to-source translator or a compiler front end —
retrofitting losslessness later is expensive, so it is designed in from the
start.

Two modes select how much the stream carries:

| Mode | Carries | For |
|---|---|---|
| `Lossless` (default) | every byte | compilers, formatters, translation, extraction that must rebuild source |
| `Lite` | code tokens only | evidence extraction, where trivia is discarded immediately anyway |

Spans stay byte-exact in both modes; only trivia presence differs.

## Languages

Lexical rules are data rather than a hand-written scanner per language, so one
tokenizer stays correct for all of them, and the differences that actually
matter are stated explicitly: nested block comments in Rust, raw strings with
hash delimiters, triple-quoted Python strings, SQL's doubled-quote escape,
significant indentation, whether a slash opens a regular expression or divides,
and whether a quote opens a character literal or a lifetime.

Structural extraction — declarations, imports and references with spans —
covers JavaScript, TypeScript, Python, Rust, Go, Java, C#, C, C++, SQL,
Solidity, HTML and CSS/SCSS/Less. Bash and YAML are tokenized but have no
structural model yet.

React is not a separate language here but is the case where the lexer's
assumptions are most fragile, so it is pinned by test: inside JSX a `/` must
stay a division rather than opening a regular expression that would swallow
the rest of the file, and `<` must not be read as a comparison.

HTML and CSS earn their place by producing an edge neither can produce alone.
A stylesheet declares selectors; a document's `class` and `id` attributes use
them; the two resolve to each other. Nesting is read from tokens rather than
flat rules, so `.card { &__title { } }` declares `.card__title` — a name that
appears nowhere in the source as written.

The brace-scoped languages share one walk driven by keyword tables, so adding
one of them costs a table rather than a scanner: Solidity was added for two
tables and a test. A language with its own scoping model costs a module, as
Python and SQL each have.

## Measured against tree-sitter

Same corpus, same machine, both sides measured in the same interleaved rounds so
machine load cannot favour either. `extract` is tokens plus facts; `ts walk` is
tree-sitter parsing plus one traversal, which is the cheapest way a consumer can
actually reach the same facts. Full method and caveats in
[docs/comparison.md](docs/comparison.md).

| language | files | extract | ts parse + walk | ratio |
|---|---|---|---|---|
| javascript | 1047 | 40.3 MB/s | 4.0 MB/s | 10.1× |
| typescript | 592 | 41.9 | 4.4 | 9.6× |
| python | 1066 | 131.5 | 8.7 | 15.1× |
| rust | 1117 | 22.3 | 2.1 | 10.8× |
| go | 880 | 45.7 | 4.2 | 11.0× |
| java | 399 | 24.7 | 3.3 | 7.6× |

Speed is worth nothing if the facts are wrong, so the same corpus is compared on
what each side finds. Imports are the fact to compare on, because every grammar
marks them with a dedicated node type.

| language | tree-sitter | ours | missed | agreement |
|---|---|---|---|---|
| javascript | 1471 | 3832 | 2 | 99.9% |
| typescript | 2373 | 2627 | 7 | 99.7% |
| python | 5814 | 5822 | 0 | 100.0% |
| rust | 5654 | 5657 | 0 | 100.0% |
| go | 5080 | 5080 | 0 | 100.0% |
| java | 4586 | 4586 | 0 | 100.0% |

Where our count exceeds tree-sitter's, the surplus was read rather than assumed:
in JavaScript it is `require()`, which `import_statement` cannot see, and in
TypeScript it is `typeof import("...")` in type positions. Both are real
dependencies.

The comparison earns its keep by finding defects, not by producing a table. It
is what surfaced a character literal holding a quote — `['.', '"', '+']` —
leaving the tokenizer treating that double quote as opening a string that ran on
for hundreds of lines and swallowed every declaration after it. Rust agreement
went from 97.2% to 100% once that and `pub(crate) use` were fixed.

Reproduce with `tools/competitor-bench`, a workspace kept outside the published
crate so tree-sitter's C grammars never reach it:

```bash
cargo run --release --manifest-path tools/competitor-bench/Cargo.toml -- <corpus-dir>
```

## What this does not do yet

It does not build a syntax tree. Facts are enough for a dependency graph and are
not enough for a compiler, for deciding which parts of a JavaScript codebase can
move to WebAssembly, or for translating Go to Rust — all of which need
expression structure. That is a third mode on the same tokenizer and is being
built; it is not claimed as done.

Nor does it reparse incrementally, or offer a query language for shapes the
crate author did not anticipate. tree-sitter does both, and does them well.
[docs/comparison.md](docs/comparison.md) states the gaps without softening them.

## License

MIT
