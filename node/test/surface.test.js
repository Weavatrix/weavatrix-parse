'use strict'

// Exercises every exported member on every advertised language, so a broken
// or missing surface fails here rather than in a consumer.

const assert = require('node:assert/strict')
const test = require('node:test')
const parse = require('..')

const SAMPLES = {
  javascript: "import fs from 'node:fs'\nexport function run() { return fs.read() }\n",
  typescript: "import type { A } from './a'\nexport const run = (value: A): number => value.size\n",
  graphql: 'type Query { user(id: ID!): User }\n',
  protobuf: 'syntax = "proto3";\npackage demo;\nmessage Ping { string value = 1; }\n',
  rust: 'pub fn run(value: u32) -> u32 {\n    value + 1\n}\n',
  python: 'import os\n\ndef run(value):\n    return os.path.join(value, "x")\n',
  go: 'package main\n\nimport "fmt"\n\nfunc Run() { fmt.Println("x") }\n',
  java: 'package demo;\n\npublic class Runner {\n  public int run() { return 1; }\n}\n',
  csharp: 'namespace Demo;\n\npublic class Runner {\n  public int Run() => 1;\n}\n',
  c: '#include <stdio.h>\n\nint run(int value) { return value + 1; }\n',
  cpp: '#include <string>\n\nnamespace demo { int run(int value) { return value + 1; } }\n',
  sql: 'CREATE TABLE users (id INT PRIMARY KEY, name TEXT);\n',
  solidity: 'pragma solidity ^0.8.0;\n\ncontract Vault { function run() public {} }\n',
  swift: 'import Foundation\n\npublic func run(value: Int) -> Int { return value + 1 }\n',
  terraform: 'resource "aws_s3_bucket" "assets" {\n  bucket = "demo"\n}\n',
  html: '<!doctype html>\n<html><body><p id="x">text</p></body></html>\n',
  xml: '<?xml version="1.0"?>\n<root><child name="x">text</child></root>\n',
  markdown: '# Title\n\nSome prose with `code` and a [link](https://example.com).\n',
  mdx: "import { Chart } from './chart'\n\n# Title\n\n<Chart value={1} />\n",
  rst: 'Title\n=====\n\nSome prose with ``code``.\n',
  asciidoc: '= Title\n\nSome prose with `code`.\n',
  css: '.card { color: #fff; margin: 0 auto; }\n',
  scss: '$brand: #fff;\n\n.card { color: $brand; &:hover { color: #000; } }\n',
  bash: '#!/usr/bin/env bash\nset -euo pipefail\nrun() { echo "$1"; }\n',
  yaml: 'name: demo\njobs:\n  build:\n    runs-on: ubuntu-latest\n',
}

const PATHS = {
  'src/a.ts': 'typescript',
  'src/a.tsx': 'typescript',
  'src/a.mjs': 'javascript',
  'schema.graphql': 'graphql',
  'api.proto': 'protobuf',
  'src/lib.rs': 'rust',
  'main.go': 'go',
  'setup.py': 'python',
  'Program.cs': 'csharp',
  'main.cpp': 'cpp',
  'schema.sql': 'sql',
  'Vault.sol': 'solidity',
  'App.swift': 'swift',
  'main.tf': 'terraform',
  'index.html': 'html',
  'README.md': 'markdown',
  'styles.scss': 'scss',
  'deploy.sh': 'bash',
  'ci.yaml': 'yaml',
}

test('exports exactly the documented surface', () => {
  assert.deepEqual(
    Object.keys(parse).sort(),
    ['extract', 'extractPath', 'supportedLanguages', 'tokenize'],
  )
  for (const name of Object.keys(parse)) {
    assert.equal(typeof parse[name], 'function', `${name} must be callable`)
  }
})

test('every advertised language has a sample and parses', () => {
  const languages = parse.supportedLanguages()
  assert.equal(languages.length, 25)
  assert.deepEqual(languages, [...new Set(languages)], 'languages must be unique')
  assert.deepEqual(Object.keys(SAMPLES).sort(), [...languages].sort())

  for (const language of languages) {
    const source = SAMPLES[language]
    const facts = parse.extract(source, language)
    for (const key of ['declarations', 'imports', 'references', 'contracts', 'diagnostics']) {
      assert.ok(Array.isArray(facts[key]), `${language}.${key} must be an array`)
    }
    assert.deepEqual(facts.diagnostics, [], `${language} must parse without diagnostics`)
  }
})

test('lossless tokenization reproduces every sample byte for byte', () => {
  for (const [language, source] of Object.entries(SAMPLES)) {
    const tokens = parse.tokenize(source, language)
    assert.equal(tokens.map((token) => token.text).join(''), source, `${language} is not lossless`)
    for (const token of tokens) {
      assert.equal(token.text, source.slice(token.start, token.end), `${language} span mismatch`)
      assert.ok(token.line >= 1 && token.column >= 1, `${language} positions must be one-based`)
    }
    const lite = parse.tokenize(source, language, { mode: 'lite' })
    assert.ok(lite.length <= tokens.length, `${language} lite must not exceed lossless`)
    for (const token of lite) {
      assert.equal(token.text, source.slice(token.start, token.end), `${language} lite span mismatch`)
    }
  }
})

test('extractPath resolves every documented extension', () => {
  for (const [path, language] of Object.entries(PATHS)) {
    const viaPath = parse.extractPath(path, SAMPLES[language])
    assert.notEqual(viaPath, undefined, `${path} must resolve`)
    assert.deepEqual(viaPath, parse.extract(SAMPLES[language], language), `${path} must equal ${language}`)
  }
  assert.equal(parse.extractPath('asset.bin', 'x'), undefined)
})

test('language aliases and casing resolve to the same facts', () => {
  const source = SAMPLES.typescript
  const canonical = parse.extract(source, 'typescript')
  for (const alias of ['ts', '.ts', ' TS ', 'tsx', 'mts', 'cts']) {
    assert.deepEqual(parse.extract(source, alias), canonical, `${alias} must be TypeScript`)
  }
})

test('empty input is accepted and rejections are typed', () => {
  assert.deepEqual(parse.tokenize('', 'rust'), [])
  const empty = parse.extract('', 'rust')
  assert.deepEqual(empty.declarations, [])
  assert.deepEqual(empty.diagnostics, [])
  assert.throws(() => parse.extract('x', 'cobol'), { code: 'InvalidArg' })
  assert.throws(() => parse.tokenize('x', 'cobol'), /unsupported language/)
})
