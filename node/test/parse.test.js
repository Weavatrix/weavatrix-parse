'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')
const { extract, extractPath, supportedLanguages, tokenize } = require('..')

const source = `import { readFile as read } from 'node:fs'\nexport function load(path) { return read(path) }\n`

test('extracts deterministic TypeScript facts', () => {
  const facts = extract(source, 'typescript')
  assert.equal(facts.imports[0].specifier, 'node:fs')
  assert.deepEqual(facts.imports[0].bindings, [{ imported: 'readFile', local: 'read' }])
  assert.equal(facts.declarations.find((item) => item.name === 'load').exported, true)
  assert.equal(facts.references.find((item) => item.name === 'read').kind, 'call')
})

test('selects languages from paths and preserves lossless tokens', () => {
  assert.deepEqual(extractPath('src/load.ts', source), extract(source, 'typescript'))
  assert.equal(extractPath('asset.unknown', source), undefined)
  const tokens = tokenize(source, 'ts')
  assert.equal(tokens.map((token) => token.text).join(''), source)
  assert.ok(tokenize(source, 'ts', { mode: 'lite' }).length < tokens.length)
  assert.ok(supportedLanguages().includes('rust'))
})

test('rejects unknown languages', () => {
  assert.throws(() => extract('value', 'brainfuck'), /unsupported language/)
})

test('carries exact declaration spans, extents, owners, and test-only scope', () => {
  const facts = extract(
    'pub fn production() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn covered() { production(); }\n}\n',
    'rs',
  )
  const production = facts.declarations.find((item) => item.name === 'production')
  assert.deepEqual(production.span, { start: 0, end: 17, line: 1, column: 1, endLine: 1, endColumn: 8 })
  assert.deepEqual(production.extent, { start: 0, end: 22, line: 1, column: 1, endLine: 1, endColumn: 22 })
  assert.equal(production.owner, null)
  assert.equal(production.testOnly, false)
  const covered = facts.declarations.find((item) => item.name === 'covered')
  assert.equal(covered.owner, 'tests')
  assert.equal(covered.testOnly, true)
})

test('keeps typed GraphQL and Protobuf contract kinds', () => {
  const graphql = extract('type Query { user(id: ID!): User }\n', 'graphql')
  assert.deepEqual(graphql.contracts[0].kind, { type: 'graphql-type', graphqlType: 'object' })
  assert.deepEqual(graphql.contracts[1].kind, { type: 'graphql-field', operation: 'query', returnType: 'User' })
  assert.equal(graphql.contracts[1].owner, 'Query')

  const proto = extract(
    'syntax = "proto3";\npackage demo;\nservice Greeter {\n  rpc Greet (stream Ping) returns (Pong);\n}\n',
    'proto',
  )
  assert.deepEqual(proto.contracts.at(0).kind, { type: 'protobuf-package' })
  assert.deepEqual(proto.contracts.at(-1).kind, {
    type: 'protobuf-rpc',
    input: 'Ping',
    output: 'Pong',
    clientStreaming: true,
    serverStreaming: false,
  })
})
