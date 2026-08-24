// Output-equivalent comparison against the TypeScript compiler's own parser.
//
// Both sides must produce the identical extracted-fact array before either is
// timed. TypeScript 7 removed the JavaScript compiler API from the npm
// package, so 5.9.3 is the last released build that can run this contract.
//
// The sweep is deliberate: the Rust advantage is largest on ordinary source
// files and shrinks on one enormous file, where the JSON boundary rather than
// parsing dominates the measured time.
import { performance } from 'node:perf_hooks'
import ts from 'typescript'
import parsePackage from '../lib/index.js'

const { extract } = parsePackage
const sizes = (process.env.WV_PARSE_FUNCTIONS ?? '50,500,12000')
  .split(',')
  .map((value) => Number(value.trim()))
  .filter((value) => Number.isInteger(value) && value > 0)
const rounds = Number(process.env.WV_PARSE_ROUNDS || 7)

function fixture(functions) {
  return ["import { helper } from './helper'", ...Array.from({ length: functions }, (_, index) =>
    `export function fn${index}(value: number) { return helper(value + ${index}) }`), ''].join('\n')
}

function order(facts) {
  return facts.sort((left, right) =>
    left.line - right.line || left.kind.localeCompare(right.kind) || left.name.localeCompare(right.name))
}

function weavatrixFacts(source) {
  const facts = extract(source, 'typescript')
  const extracted = []
  for (const item of facts.imports) {
    extracted.push({ kind: 'import', name: item.specifier, line: item.span.line })
  }
  for (const item of facts.declarations) {
    if (item.kind === 'function') extracted.push({ kind: 'function', name: item.name, line: item.span.line })
  }
  for (const item of facts.references) {
    if (item.kind === 'call') extracted.push({ kind: 'call', name: item.name, line: item.span.line })
  }
  return order(extracted)
}

function typescriptFacts(source) {
  const file = ts.createSourceFile('fixture.ts', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS)
  const extracted = []
  const lineOf = (node) => file.getLineAndCharacterOfPosition(node.getStart(file)).line + 1
  function visit(node) {
    if (ts.isImportDeclaration(node)) {
      extracted.push({ kind: 'import', name: node.moduleSpecifier.text, line: lineOf(node) })
    } else if (ts.isFunctionDeclaration(node) && node.name) {
      extracted.push({ kind: 'function', name: node.name.text, line: lineOf(node) })
    } else if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
      extracted.push({ kind: 'call', name: node.expression.text, line: lineOf(node.expression) })
    }
    ts.forEachChild(node, visit)
  }
  visit(file)
  return order(extracted)
}

function time(operation) {
  const start = performance.now()
  operation()
  return performance.now() - start
}

function median(samples) {
  const sorted = [...samples].sort((left, right) => left - right)
  return Number(sorted[Math.floor(sorted.length / 2)].toFixed(3))
}

function measurePair(left, right) {
  const leftSamples = []
  const rightSamples = []
  for (let round = 0; round < rounds + 2; round += 1) {
    let leftElapsed
    let rightElapsed
    if (round % 2 === 0) {
      leftElapsed = time(left)
      rightElapsed = time(right)
    } else {
      rightElapsed = time(right)
      leftElapsed = time(left)
    }
    if (round >= 2) {
      leftSamples.push(leftElapsed)
      rightSamples.push(rightElapsed)
    }
  }
  return [median(leftSamples), median(rightSamples)]
}

const results = sizes.map((functions) => {
  const source = fixture(functions)
  const facts = weavatrixFacts(source)
  if (JSON.stringify(facts) !== JSON.stringify(typescriptFacts(source))) {
    throw new Error(`extracted-fact parity failed at ${functions} functions`)
  }
  const [weavatrixMs, typescriptMs] = measurePair(
    () => weavatrixFacts(source),
    () => typescriptFacts(source),
  )
  return {
    functions,
    sourceBytes: Buffer.byteLength(source),
    extractedFacts: facts.length,
    weavatrixMs,
    typescriptMs,
    ratio: Number((typescriptMs / weavatrixMs).toFixed(2)),
  }
})

console.log(JSON.stringify({
  runtime: process.versions.bun ? `bun ${process.versions.bun}` : `node ${process.version}`,
  typescript: ts.version,
  contract: 'identical extracted fact array: imports, function declarations, and calls with names and one-based lines',
  rounds,
  results,
}, null, 2))
