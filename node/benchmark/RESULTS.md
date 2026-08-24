# Node.js and Bun benchmark snapshot

Measured on 2026-08-24 on Windows x64. Both sides must return the identical
extracted-fact array — every import specifier, every function declaration, and
every call, each with its name and one-based line — before either is timed.
Values are medians of seven measured rounds after two warm-up rounds, with
execution order alternating per round.

The competitor is the TypeScript compiler's own parser at 5.9.3. TypeScript 7
removed the JavaScript compiler API from the npm package, so 5.9.3 is the last
released build that can run this contract in-process.

| Source | Facts | Runtime | Weavatrix | TypeScript 5.9.3 | Result |
| --- | ---: | --- | ---: | ---: | ---: |
| 3,314 B | 101 | Node 24.15.0 | 0.531 ms | 3.633 ms | Weavatrix 6.84x faster |
| 33,814 B | 1,001 | Node 24.15.0 | 5.389 ms | 10.510 ms | Weavatrix 1.95x faster |
| 841,814 B | 24,001 | Node 24.15.0 | 92.754 ms | 118.313 ms | Weavatrix 1.28x faster |
| 3,314 B | 101 | Bun 1.3.14 | 0.494 ms | 2.874 ms | Weavatrix 5.82x faster |
| 33,814 B | 1,001 | Bun 1.3.14 | 4.197 ms | 6.838 ms | Weavatrix 1.63x faster |
| 841,814 B | 24,001 | Bun 1.3.14 | 66.223 ms | 79.453 ms | Weavatrix 1.20x faster |

The sweep is the point. On ordinary source files the Rust tokenizer and
extractor dominate and the margin is large. On one enormous file the margin
narrows, because the measured time stops being parsing and becomes the JSON
boundary: at 841,814 source bytes this contract materializes a 5.6 MB fact
document, and encoding it in Rust plus `JSON.parse` in JavaScript costs more
than the extraction itself. Repository-scale consumers see the upper rows,
because they call `extract` once per file.

These rows measure fact extraction, not tokenization, and they compare only
TypeScript. `weavatrix-parse` covers 25 languages with one dependency-free
engine; the TypeScript compiler covers one.

Reproduce from `node/`:

```console
npm ci
npm run build
npm run bench
bun run benchmark/typescript.mjs
```

`WV_PARSE_FUNCTIONS` overrides the size sweep (a comma-separated list) and
`WV_PARSE_ROUNDS` the measured round count. CPU, memory bandwidth, and
JavaScript engine version can materially change these timings. Treat them as a
reproducible snapshot, not a universal result.
