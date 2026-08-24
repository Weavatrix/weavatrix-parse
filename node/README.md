# weavatrix-parse

A lossless source tokenizer and structural fact extractor for 25 languages,
written in Rust and exposed to Node.js and Bun through Node-API.

It answers one question per file: *what does this source declare, import,
reference, and expose?* — with exact byte offsets, without building a syntax
tree, without a C toolchain, and without executing the source.

```console
npm install weavatrix-parse
# or
bun add weavatrix-parse
```

```js
const { extract, tokenize } = require('weavatrix-parse')

const facts = extract("import api from './api'\nexport function run() { api() }", 'typescript')
facts.imports[0].specifier      // './api'
facts.declarations[0].name      // 'run'
facts.declarations[0].exported  // true
facts.references[0].kind        // 'call'

tokenize('const answer = 42', 'javascript').map((token) => token.text).join('')
// 'const answer = 42' — every byte belongs to exactly one token
```

ESM works the same way:

```js
import parse from 'weavatrix-parse'
const { extract, tokenize, extractPath, supportedLanguages } = parse
```

---

## Two ideas worth knowing before the API

**Lossless.** Every byte of input belongs to exactly one token, whitespace and
comments included, so concatenating token texts reproduces the source exactly.
That is what lets the same stream serve a formatter, a translator, and an
extractor.

**Facts, not a tree.** Extraction returns five flat arrays — declarations,
imports, references, contracts, diagnostics — each entry carrying an exact
span. That is enough for a dependency graph, an API map, or an impact
analysis, and it is deliberately not enough for a compiler.

---

## API

### `extract(source, language) → Facts`

Extracts structural facts from `source`.

| Parameter | Type | Notes |
| --- | --- | --- |
| `source` | `string` | The complete file text. Empty input is valid and returns empty arrays. |
| `language` | `string` | A language name or any alias below. Leading dots and surrounding whitespace are ignored, and matching is case-insensitive. |

Throws `InvalidArg` for an unknown language.

### `extractPath(path, source) → Facts | undefined`

Same as `extract`, but chooses the language from the file extension in `path`.
Returns `undefined` when the extension maps to no supported language, so a
caller can walk a repository and skip unknown files without a try/catch.

```js
extractPath('src/router.ts', source)   // TypeScript facts
extractPath('assets/logo.png', source) // undefined
```

### `tokenize(source, language, options?) → Token[]`

Returns the token stream.

| Option | Type | Default | Effect |
| --- | --- | --- | --- |
| `mode` | `'lossless' \| 'lite'` | `'lossless'` | `lossless` emits every byte, including whitespace and comments. `lite` emits code tokens only, and spans stay byte-exact in both modes. |

### `supportedLanguages() → string[]`

The 25 canonical language names, in a stable order.

---

## Languages

| Canonical name | Accepted aliases |
| --- | --- |
| `javascript` | `js`, `jsx`, `mjs`, `cjs` |
| `typescript` | `ts`, `tsx`, `mts`, `cts` |
| `graphql` | `gql` |
| `protobuf` | `proto` |
| `rust` | `rs` |
| `python` | `py`, `pyi` |
| `go` | — |
| `java` | — |
| `csharp` | `cs` |
| `c` | `h` |
| `cpp` | `c++`, `cc`, `cxx`, `hpp` |
| `sql` | `psql` |
| `solidity` | `sol` |
| `swift` | — |
| `terraform` | `tf`, `hcl` |
| `html` | `htm` |
| `xml` | — |
| `markdown` | `md` |
| `mdx` | — |
| `rst` | `restructuredtext` |
| `asciidoc` | `adoc` |
| `css` | — |
| `scss` | `sass`, `less` |
| `bash` | `sh`, `zsh` |
| `yaml` | `yml` |

---

## Returned shapes

### `Span`

Every fact carries one. Offsets are byte offsets into `source`; lines and
columns are one-based.

```ts
{ start: number, end: number, line: number, column: number, endLine: number, endColumn: number }
```

### `Facts`

```ts
{
  declarations: Declaration[]
  imports: ImportFact[]
  references: Reference[]
  contracts: Contract[]
  diagnostics: ParseDiagnostic[]
}
```

### `Declaration`

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | `string` | The declared name. |
| `kind` | `string` | `function`, `method`, `class`, `interface`, `enum`, `type-alias`, `field`, `constant`, `variable`, `module`, `struct`, `trait`, `table`, `view`, `procedure`, `selector`, `resource`, `heading`, or `unknown`. |
| `span` | `Span` | The name and its modifiers. |
| `extent` | `Span` | The whole declaration including its body. Comparing `extent` lets a consumer detect a changed function body without storing source text. |
| `owner` | `string \| null` | The enclosing declaration, when the language nests them. |
| `exported` | `boolean` | Whether the declaration leaves the module. |
| `testOnly` | `boolean` | Whether it exists only in a test compilation. Rust fills this from `#[test]` and positive `#[cfg(test)]`; languages without compile-time test scopes always report `false`. |

### `ImportFact`

| Field | Type | Meaning |
| --- | --- | --- |
| `specifier` | `string` | Exactly as written, without quotes. |
| `span` | `Span` | |
| `typeOnly` | `boolean` | A type-position import, which disappears when compiled. |
| `reexport` | `boolean` | `export … from`, which forwards another module's surface. |
| `names` | `string[]` | Local names this import binds. |
| `bindings` | `{ imported: string, local: string }[]` | Lossless pairs, so `import { original as local }` still resolves to `original`. |

### `Reference`

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | `string` | The referenced name, without its receiver. |
| `kind` | `string` | `call`, `inherits`, `implements`, `uses`, `reads`, `writes`, or `unknown`. |
| `receiver` | `string \| null` | Whatever was written before the final dot. |
| `span` | `Span` | |
| `owner` | `string \| null` | The enclosing declaration the reference was written in. |
| `stringArguments` | `string[]` | Literal string arguments, which carry routes, topics, and table names. |
| `nameArguments` | `string[]` | Names passed as arguments, in written order, so `app.use("/api", router)` resolves both ends. |

### `Contract`

Typed transport facts. `kind.type` selects the shape:

| `kind.type` | Extra fields |
| --- | --- |
| `graphql-type` | `graphqlType`: `object`, `interface`, `input`, `enum`, `scalar`, `union` |
| `graphql-field` | `operation`: `query` \| `mutation` \| `subscription` \| `null`; `returnType`: `string` |
| `graphql-operation`, `graphql-call` | `operation` |
| `graphql-fragment` | `onType`: `string`; `operation` |
| `graphql-fragment-spread` | — |
| `protobuf-package`, `protobuf-message`, `protobuf-enum`, `protobuf-service` | — |
| `protobuf-rpc` | `input`, `output`: `string`; `clientStreaming`, `serverStreaming`: `boolean` |

Each contract also carries `name`, `span`, and `owner`.

### `ParseDiagnostic`

`{ code: string, message: string, span: Span }`. The extractor fails closed: it
emits a diagnostic instead of guessing a structural fact.

### `Token`

`{ kind: string, start: number, end: number, line: number, column: number, text: string }`

`kind` is one of `whitespace`, `newline`, `indent`, `line-comment`,
`block-comment`, `string`, `interpolation`, `number`, `identifier`, `regex`,
`punctuation`, `unterminated`, or `unknown`.

---

## Errors

Every rejection is a Node-API error with a `code`:

| `code` | Cause |
| --- | --- |
| `InvalidArg` | Unknown language name or alias. |
| `GenericFailure` | Serialization failure. |

```js
try {
  extract(source, 'cobol')
} catch (error) {
  error.code    // 'InvalidArg'
  error.message // 'unsupported language: cobol'
}
```

---

## What ships

One package, all six platforms, nothing at install time:

| | |
| --- | --- |
| Runtimes | Node.js 18+ (Node-API 8), Bun 1.4+ |
| Platforms | Windows x64/arm64, macOS x64/arm64, glibc Linux x64/arm64 |
| Install script | none |
| Network at install | none |
| Runtime dependencies | none |
| Platform packages | none — all six bindings are in this one tarball |

musl Linux is not currently built; the loader raises a clear error rather than
loading a glibc binary.

---

## Measured

[`benchmark/RESULTS.md`](benchmark/RESULTS.md) is generated from the
[weavatrix-benchmarks](https://github.com/Weavatrix/weavatrix-benchmarks)
harness, which forces both sides to return the identical extracted-fact array
before either is timed. The competitor is the TypeScript compiler's own parser
at 5.9.3 — the last release that still ships the JavaScript compiler API.

Medians of three independent runs, each in a fresh process:

| Source | Facts | Node 24 | Bun 1.3 |
| ---: | ---: | ---: | ---: |
| 3,314 B | 101 | **3.13x** (2.68–4.77) | **4.29x** (4.20–5.15) |
| 33,814 B | 1,001 | **1.16x** (1.16–1.26) | **1.59x** (1.56–1.60) |
| 841,814 B | 24,001 | **1.03x** (1.00–1.07) | **1.27x** (1.24–1.29) |

The sweep is the point. On ordinary source files the Rust tokenizer dominates.
On one enormous file the margin collapses to parity, because the measured time
stops being parsing and becomes the JSON boundary: that contract materializes a
5.6 MB fact document, and encoding it plus `JSON.parse` costs more than the
extraction. Repository-scale consumers see the upper rows, because they call
`extract` once per file.

---

Repository: [Weavatrix/weavatrix-parse](https://github.com/Weavatrix/weavatrix-parse) ·
Rust crate: [crates.io/crates/weavatrix-parse](https://crates.io/crates/weavatrix-parse) ·
License: [MIT](https://github.com/Weavatrix/weavatrix-parse/blob/main/LICENSE)
