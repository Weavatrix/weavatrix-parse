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
covers eighteen languages and formats:

| | |
|---|---|
| Curly-brace languages | JavaScript, TypeScript, Rust, Go, Java, C#, C, C++, Swift, Solidity |
| Own scoping model | Python, SQL |
| Web | HTML, CSS/SCSS/Less, Vue and Svelte components |
| Configuration and markup | Terraform/HCL, XML |
| Documents | Markdown, MDX, reStructuredText, AsciiDoc |

Bash and YAML are tokenized but have no structural model yet.

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

Same corpus, same machine, both sides measured in the same interleaved rounds so
machine load cannot favour either. `extract` is tokens plus facts; `ts walk` is
tree-sitter parsing plus one traversal, which is the cheapest way a consumer can
actually reach the same facts. Full method and caveats in
[docs/comparison.md](docs/comparison.md).

| language | files | extract | ts parse + walk | ratio |
|---|---|---|---|---|
| javascript | 1047 | 41.9 MB/s | 4.5 MB/s | 9.3× |
| typescript | 592 | 67.4 | 7.6 | 8.8× |
| python | 1064 | 105.8 | 10.0 | 10.6× |
| rust | 1128 | 40.9 | 3.9 | 10.4× |
| go | 879 | 43.2 | 4.2 | 10.4× |
| java | 399 | 51.9 | 6.8 | 7.7× |

Speed is worth nothing if the facts are wrong, so the same corpus is compared on
what each side finds. Imports are the fact to compare on, because every grammar
marks them with a dedicated node type.

| language | tree-sitter | ours | missed | agreement |
|---|---|---|---|---|
| javascript | 1471 | 3832 | 0 | 100.0% |
| typescript | 2373 | 2634 | 0 | 100.0% |
| python | 5814 | 5822 | 0 | 100.0% |
| rust | 5688 | 5691 | 0 | 100.0% |
| go | 5075 | 5075 | 0 | 100.0% |
| java | 4586 | 4586 | 0 | 100.0% |
| c# | 28 | 28 | 0 | 100.0% |

Where our count exceeds tree-sitter's, the surplus was read rather than assumed:
in JavaScript it is `require()`, which `import_statement` cannot see, and in
TypeScript it is `typeof import("...")` in type positions. Both are real
dependencies.

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
