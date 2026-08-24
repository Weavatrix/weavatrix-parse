# weavatrix-parse

The lossless tokenizer and structural fact extractor behind Weavatrix, exposed as a native library for Node.js and Bun. It is the Rust `weavatrix-parse` core through Node-API—not a JavaScript rewrite and not an MCP server.

## Install

```console
npm install weavatrix-parse
# or
bun add weavatrix-parse
```

```js
const { extract, tokenize } = require('weavatrix-parse')

const facts = extract(`import api from './api'\nexport function run() { api() }`, 'typescript')
console.log(facts.imports)
console.log(facts.declarations)

const tokens = tokenize('const answer = 42', 'javascript')
console.log(tokens.map((token) => token.text).join(''))
```

`extractPath(path, source)` selects the language from the extension. `tokenize` is lossless by default; `{ mode: 'lite' }` omits whitespace and comments while retaining exact byte spans.

## Native product boundary

One self-contained npm package supports Node.js 18+ and Bun 1.4+ and includes Windows, macOS, and glibc Linux bindings for x64 and arm64. It has no install script, performs no network download, creates no public platform-package names, and keeps tokenization and extraction in Rust.

The first Node/Bun surface covers lossless and lite tokenization plus declarations, imports, references, GraphQL/Protobuf contracts, diagnostics, exact spans, ownership, export status, and test-only Rust declarations.

## Measured

[`benchmark/RESULTS.md`](benchmark/RESULTS.md) compares an identical extracted-fact array against the TypeScript compiler's own parser at 5.9.3. Weavatrix won every measured row, by 6.84x on a 3,314-byte file down to 1.28x on a single 841,814-byte file where the JSON boundary rather than parsing dominates.

Repository: [Weavatrix/weavatrix-parse](https://github.com/Weavatrix/weavatrix-parse) · Rust crate: [crates.io/crates/weavatrix-parse](https://crates.io/crates/weavatrix-parse) · License: [MIT](https://github.com/Weavatrix/weavatrix-parse/blob/main/LICENSE)
