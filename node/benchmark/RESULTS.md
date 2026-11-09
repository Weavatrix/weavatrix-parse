# Node.js and Bun benchmark snapshot

This file is generated. Every number below was produced by the
[weavatrix-benchmarks](https://github.com/Weavatrix/weavatrix-benchmarks)
harness and copied out of its recorded run; none of it is typed by hand.
That repository states the rules every suite obeys, including what each
row had to prove equal before it was allowed to be timed.

**Question.** How fast is extracting imports, function declarations, and calls with exact positions?

**Competitor.** `typescript 5.9.3`

| Property | Value |
| --- | --- |
| Measured | 2026-08-24 |
| Platform | win32 x64, 10.0.26200 |
| CPU | Intel(R) Core(TM) Ultra 7 255U (14 logical cores) |
| Memory | 47.5 GiB |
| Rounds | 7 measured, after 2 warm-ups, alternating order, median reported |
| Independent runs | 3 per suite, each in a fresh process; the table shows the median and the spread |
| Package | weavatrix-parse 0.3.4 |

## node 24.15.0

Corpus: `[{"sizes":[50,500,12000]}]`

| Contract | Parity | Weavatrix | Competitor | Result |
| --- | --- | ---: | ---: | ---: |
| 3,314 source bytes, 101 facts | identical {kind, name, line} array | 0.501 ms | 1.582 ms | Weavatrix 3.13x faster (2.68x–4.77x) |
| 33,814 source bytes, 1,001 facts | identical {kind, name, line} array | 2.939 ms | 3.503 ms | Weavatrix 1.16x faster (1.16x–1.26x) |
| 841,814 source bytes, 24,001 facts | identical {kind, name, line} array | 66.511 ms | 68.522 ms | Weavatrix 1.03x faster (1.00x–1.07x) |

## bun 1.3.14

Corpus: `[{"sizes":[50,500,12000]}]`

| Contract | Parity | Weavatrix | Competitor | Result |
| --- | --- | ---: | ---: | ---: |
| 3,314 source bytes, 101 facts | identical {kind, name, line} array | 0.269 ms | 1.386 ms | Weavatrix 4.29x faster (4.20x–5.15x) |
| 33,814 source bytes, 1,001 facts | identical {kind, name, line} array | 2.827 ms | 4.405 ms | Weavatrix 1.59x faster (1.56x–1.60x) |
| 841,814 source bytes, 24,001 facts | identical {kind, name, line} array | 60.766 ms | 77.281 ms | Weavatrix 1.27x faster (1.24x–1.29x) |

## Reading these rows

- TypeScript 7 removed the JavaScript compiler API from the npm package, so 5.9.3 is the last released build that can run this contract in process.
- **841,814 source bytes, 24,001 facts** — the margin narrows on one enormous file because the measured time stops being parsing and becomes the JSON boundary

## Reproduce

```console
git clone https://github.com/Weavatrix/weavatrix-benchmarks
cd weavatrix-benchmarks && npm ci
node run.mjs --suite=parse
bun run.mjs --suite=parse
node export.mjs
```

CPU, memory bandwidth, filesystem, antivirus, and JavaScript engine
version all move these timings. Treat them as a reproducible snapshot of
the environment above, not as a universal result.
