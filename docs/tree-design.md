# The syntax tree: what was decided, and what was rejected

Four designs were written independently and judged on three lenses —
feasibility under zero dependencies and forbidden `unsafe`, measured
performance against the existing fact path, and fitness for the consumers that
actually exist. This records the outcome so the settled questions are not
reopened.

## The consumers decided it

None of the four named consumers is an editor. weavatrix-rust scans monorepos;
a compiler wants a faithful tree; a library decides which parts of a JavaScript
codebase can move to WebAssembly; another translates Go to Rust. The two hard
ones both need the same two properties, and they are what the representation
was chosen for:

`&source[node.range()]` must be one slice. A translator that cannot lower a
construct copies its original bytes verbatim, so a node must know its absolute
byte range without reconstructing it from ancestors.

A subtree must be a contiguous index range. "Does this function body contain
`await`" then becomes a linear scan over a fixed-stride slice with no
allocation, and node indices can key dense side tables — which is how the
WebAssembly analyzer works.

The incrementality-first design scored well and was still not chosen, because
its defining move — dropping absolute offsets and parent pointers so a splice
rewrites nothing — costs exactly those two properties.

Incremental reparse is nevertheless a requirement, not a hypothetical: an
editor and a language server are stated consumers. What the choice costs is
stated plainly rather than hidden. With absolute offsets, an edit shifts the
offsets of every node after it, so a splice is O(nodes after the edit) instead
of O(edit). For a file that is a `memmove` over a few thousand 20-byte records
and a bounded add over their offsets — cheap in absolute terms, and paid per
keystroke rather than per scan. The alternative would have made every
translator lookup and every analyzer side table more expensive, permanently, to
make that one operation asymptotically better. Two grafts were taken
specifically to keep the cost down: a relative parent index, so a node and its
parent that shift together need no fixup at all, and `LexState` with
`Tokenizer::resume()`, so relexing restarts at the edit rather than at the file
start.

`reparse` returns a report of what it reused, so the claim is measurable in a
benchmark and falsifiable in a test rather than asserted here.

## Shape

A flat pre-order `Vec<Node>` at 20 bytes per node: kind, a role byte, flags,
absolute `start` and `end`, a descendant count, and a **relative** parent index.
Relative rather than absolute so that a node and its parent shifting together
after an edit need no fixup — the same four bytes, strictly better.

Explicit token leaves, not implicit ones. Treating tokens as ranges rather than
nodes would halve the array, and was the largest memory decision on the table.
It was rejected because it gives tokens a different address space from nodes,
which breaks the uniform side table and the copy-this-element-verbatim model,
and because it makes one iterator simultaneously responsible for span
computation, losslessness and error placement — a silent-corruption generator
in release builds. Roughly twice the nodes buys a model where every syntactic
element has exactly one address.

## Losslessness is enforced by the builder, not by discipline

The builder's cursor is a monotone token index. There is no `skip`, no `seek`,
no `set_span`, no way to name a token out of order, and `finish()` asserts the
cursor consumed every token — in release as well as debug. A grammar author
therefore cannot write a rule that drops bytes, rather than being trusted not
to.

## Error recovery

A parser that gives up at the first error is useless on real source. Errors
become `Error` nodes holding the tokens that could not be interpreted, and
`Missing` nodes stand in for required syntax that is absent, so the tree stays
a complete partition of the file either way. A `HasError` flag on every
ancestor makes "is this subtree trustworthy" an O(1) test — which is precisely
what a translator asks before lowering a construct rather than copying it.

Depth and node limits are recovered from, not fatal: a monorepo scan meets
minified bundles, and the honest answer there is a truncated tree with a
diagnostic rather than an aborted scan.

## Query

Not S-expressions. tree-sitter needs them because its tree is untyped; ours is
not. Patterns name kinds against a static table and are resolved **before**
matching, so a misspelled kind returns `UnknownKind { name, suggestion }`
instead of tree-sitter's failure mode of silently zero results. Both a compiled
Rust form and a runtime string form exist, because the MCP surface needs to
accept a query it did not compile.

## Modes

The tokenizer stays the single front end. `Lite` remains exactly what it is
today and must not pay for any of this — the fact path runs at 22–131 MB/s and
a regression there blocks the merge rather than earning a footnote. Adding
thousands of lines perturbs inlining under thin LTO, so "structurally cannot
regress" is not an argument; the benchmark is.

## Stages

Each lands on its own with fmt, clippy and the full suite green.

1. **Foundations.** Lift the keyword tables out of `braced.rs` into their own
   module; add a line index; add `LexState` with `Tokenizer::state()` and
   `resume()`. Proven by: every existing test unchanged, the line index
   agreeing with every token's recorded position in all twelve languages, and
   resuming from every token boundary reproducing a byte-identical tail.
2. **Operator composition.** The tokenizer emits single-character punctuation,
   so `=>` is two tokens — a fact this crate already works around by hand in
   the JavaScript extractor. A per-language longest-match table composes runs
   into operators, requiring byte adjacency so `a & & b` stays two operators
   while `a && b` becomes one. Plus a bracket-mate table in one stack pass.
3. **Tree core and a skeleton grammar for all twelve languages**, with error,
   missing and unclosed nodes and the depth limits. Proven by leaf
   concatenation reproducing every file of the corpus byte for byte.

Later stages add expression grammar per language, then incremental reparse —
which returns a report of what was reused, so the claim is measurable in a
benchmark and falsifiable in a test rather than asserted in a README.
