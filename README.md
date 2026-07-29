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

Structural extraction — declarations, imports, references and typed transport
contracts with byte spans — covers the following languages and formats:

| | |
|---|---|
| Curly-brace languages | JavaScript, TypeScript, Rust, Go, Java, C#, C, C++, Swift, Solidity |
| Own scoping model | Python, SQL |
| Contract schemas | GraphQL SDL/operations, Protocol Buffers/proto3 |
| Web | HTML, CSS/SCSS/Less, Vue and Svelte components |
| Configuration and markup | Terraform/HCL, XML |
| Documents | Markdown, MDX, reStructuredText, AsciiDoc |
| Shell | Bash, sh, zsh |

YAML is tokenized but has no structural model yet.

## What this crate does not decide

It extracts what the source says, not what a framework means by it.
`mongoose.model("User", schema)` comes out as a call with receiver
`mongoose`, name `model` and string argument `User`; `@KafkaListener(topics =
"orders")` as a call named `KafkaListener` with argument `orders`;
`modelBuilder.Entity<Order>().ToTable("orders")` as a use of the type `Order`
beside a call carrying `orders`. Turning those into "this file declares a
collection", "this file subscribes to a topic" or "this entity maps to that
table" is framework knowledge — it changes with library versions and there
are hundreds of libraries — so it belongs in the consumer, and this crate
stays a language layer that does not need updating when a library does.

React is not a separate language here but is the case where the lexer's
assumptions are most fragile, so it is pinned by test: inside JSX a `/` must
stay a division rather than opening a regular expression that would swallow
the rest of the file, and `<` must not be read as a comparison.

Several of these earn their place by producing an edge no other extractor can.
A stylesheet declares selectors and a document's `class` and `id` attributes
use them, so the two resolve to each other — and nesting is read from tokens
rather than flat rules, so `.card { &__title { } }` declares `.card__title`, a
name that appears nowhere in the source as written. Terraform ties
infrastructure into the same graph: a `module` names another directory, and
every `var.x`, `module.m.out` and `aws_s3_bucket.b.id` is a reference between
declared objects. Documents contribute their heading tree and every link that
points at a path in the repository rather than at the web.

Vue and Svelte components are read through their script and style blocks,
because a component keeps its imports there and nowhere else. Claiming the
extension while reading only the template would make the file a graph node
with no dependencies — which reads as "this component imports nothing" rather
than as "unsupported", and that is worse than not claiming it.

The brace-scoped languages share one walk driven by keyword tables, so adding
one of them costs a table rather than a scanner: Solidity was added for two
tables and a test. A language with its own scoping model costs a module, as
Python and SQL each have.

## Measured against tree-sitter

Same immutable input, same process and interleaved order for both sides. After
one warm-up, the table reports the median of seven measured rounds. Each
language is capped at 8 MiB so one vendored tree cannot dominate. `extract` is
tokens plus facts; `ts walk` is tree-sitter parsing plus one traversal, which is
the cheapest way a tree-sitter consumer can reach structural facts. Full method
and caveats are in [docs/comparison.md](docs/comparison.md).

| language | files | MiB | tokenize | extract | ts parse + walk | ratio | interpretation |
|---|---:|---:|---:|---:|---:|---:|---|
| JavaScript | 720 | 8.0 | 129.4 MB/s | 58.6 MB/s | 6.3 MB/s | 9.31x | measured corpus |
| TypeScript | 1789 | 8.0 | 100.3 | 38.5 | 3.6 | 10.68x | measured corpus |
| Python | 1036 | 7.6 | 174.2 | 112.4 | 12.3 | 9.12x | measured corpus |
| Rust | 838 | 5.1 | 95.9 | 42.0 | 4.9 | 8.49x | measured corpus |
| Java | 389 | 2.4 | 107.0 | 49.7 | 7.3 | 6.77x | measured corpus |
| XML | 20 | 5.3 | 95.7 | 34.8 | 2.2 | 15.70x | byte-heavy, only 20 files |
| Go | 40 | 0.2 | 105.4 | 56.5 | 5.3 | 10.58x | small corpus; no speed claim |
| C | 23 | 0.2 | 114.4 | 56.7 | 4.0 | 14.21x | small corpus; no speed claim |
| C++ | 3 | <0.1 | 85.7 | 36.6 | 2.3 | 15.99x | small corpus; no speed claim |
| SQL | 4 | <0.1 | 103.3 | 56.1 | 5.2 | 10.80x | small corpus; no speed claim |
| Bash | 33 | <0.1 | 165.5 | 75.1 | 7.4 | 10.19x | small corpus; no speed claim |

GraphQL, protobuf, C#, Swift and Terraform had no files in this selected corpus,
so there is no throughput result for them. GraphQL and protobuf correctness is
instead covered by exact typed fixtures. Protobuf accepts proto2, proto3, and
Editions 2023/2024 (including Edition 2024 `import option`) while preserving
every byte and extracting typed package/message/enum/service/RPC facts. The
measured code-language range is
currently 6.77x to 10.68x, not 30x; a 30x target remains unfulfilled.
The machine-readable table is checked in as
`benchmark-results/competitor-median-2026-07-29.txt` (SHA-256
`8C25F0BD16EC8A458B0ACEADF9D26D6E9E7AB0F6AD3CE6329870AD66E993CCCD`).

Markdown is the one exception to the second rule, and it is not a result to be
proud of: prose has no token structure, so the document extractor reads lines
directly and never tokenizes at all. It is fast because it does far less than
tree-sitter does, not because it does the same thing faster.

Speed is worth nothing if the facts are wrong, so the same corpus is compared on
what each side finds. Imports are the fact to compare on, because every grammar
marks them with a dedicated node type.

| language | tree-sitter | ours | missed | agreement |
|---|---|---|---|---|
| javascript | 2872 | 3081 | 0 | 100.0% |
| typescript | 9579 | 9758 | 0 | 100.0% |
| python | 5668 | 5671 | 0 | 100.0% |
| rust | 3942 | 3943 | 0 | 100.0% |
| go | 277 | 277 | 0 | 100.0% |
| java | 4586 | 4586 | 0 | 100.0% |

This proves zero misses against tree-sitter's dedicated import nodes on this
corpus. It does not by itself prove that every surplus fact is correct:
`require()` and type-position imports are expected examples, while the full
surplus remains subject to source review.

The comparison earns its keep by finding defects, not by producing a table.
Every one of these was found by running it and read in the source before being
fixed:

A character literal holding a quote — `['.', '"', '+']` — left the tokenizer
treating that double quote as opening a string, which ran on for hundreds of
lines and swallowed every declaration after it. `pub(crate) use x;` was
invisible because the import path stepped over modifiers but not over a
parenthesised visibility scope. `#include <stdio.h>` scanned past the end of
its line and consumed the function beneath it. `int add(int a, int b) { }`
matched no declaration rule and fell through to the call path, so every C
function definition was recorded as a call to itself. And
`import service, { helper } from './x'` was dropped by a guard that broke at
any brace which was not the second token — the one shape a default name before
the brace produces.

Rust went 97.2% → 100%, JavaScript and TypeScript 99.9% and 99.7% → 100%.

Reproduce with `tools/competitor-bench`, a workspace kept outside the published
crate so tree-sitter's C grammars never reach it:

```bash
cargo run --release --manifest-path tools/competitor-bench/Cargo.toml -- \
  --output target/competitor.txt <corpus-dir>...
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
