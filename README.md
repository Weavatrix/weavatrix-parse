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

It is not a compiler front end for its own sake: it does not build expression
trees, because repository intelligence consumes declarations, imports, exports,
calls and spans, not operator precedence. That boundary is what keeps the crate
small enough to own outright and fast enough to run over a monorepo.

## Lossless by default

Every byte of the input belongs to exactly one token, whitespace and comments
included, so concatenating the token texts reproduces the source exactly. The
test suite asserts that invariant. It is what lets the same stream serve a
formatter, a source-to-source translator or a compiler front end - retrofitting
losslessness later is expensive, so it is designed in from the start.

Two modes select how much the stream carries:

| Mode | Carries | For |
|---|---|---|
| `Lossless` (default) | every byte | compilers, formatters, translation, extraction that must rebuild source |
| `Lite` | code tokens only | evidence extraction, where trivia is discarded immediately anyway |

Spans stay byte-exact in both modes; only trivia presence differs.

## Languages

The lexical rules of each language are data, not a hand-written scanner per
language, so one tokenizer stays correct for all of them: JavaScript,
TypeScript, Rust, Python, Go, Java, C#, C, C++, SQL, Bash and YAML.

The differences that actually matter are described explicitly - nested block
comments in Rust, raw strings with hash delimiters, triple-quoted Python
strings, SQL's doubled-quote escape, significant indentation, and whether a
slash opens a regular expression or divides.

## Status

The tokenizer is complete and tested. Structural extraction per language is
being moved here from the Weavatrix engine.

## License

MIT
