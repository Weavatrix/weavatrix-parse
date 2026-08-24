# Changelog

## Unreleased

- Publish one self-contained `weavatrix-parse` npm package for Node.js 18+ and
  Bun 1.4+, carrying all six native bindings (Windows, macOS, and glibc Linux
  on x64 and arm64) in a single tarball with no install script, no download,
  and no public platform-package names.
- Expose `extract`, `extractPath`, `tokenize`, and `supportedLanguages` with
  TypeScript types over the same Rust engine; the JavaScript layer owns only
  the loader and JSON decoding.
- Serialize facts and tokens from borrowed views instead of an intermediate
  value tree, which cut the native side of a 24,001-fact extraction from
  156 ms to 45 ms and lite tokenization of the same source from 291 ms to
  45 ms.
- Add the `Node and Bun native bindings` and `Publish npm package` workflows
  and an output-equivalent benchmark against the TypeScript compiler parser.

## 0.3.2 - 2026-08-23

- Swift type heritage is taken from colon lists on types only: the first type
  on a class inherits, later types and every colon type on a struct, enum,
  protocol or extension implement, and parameter labels such as
  `pairing: Pairing` stay ordinary names;
- interpolated Swift strings yield route fragments
  (`"\(base)/pair/\(mailbox)"` produces `/pair`);
- assignments such as `request.httpMethod = "PUT"` and `comps.path = "/ws"`
  are recorded as call facts;
- six Swift fact tests, all parser tests, and strict Clippy.

## 0.3.0 - 2026-08-03

- emit dependency-injection wiring as `Uses` references: Java and C# field
  declaration types and constructor parameter types (Spring `@Autowired`
  fields, constructor injection), and TypeScript constructor parameter and
  class-field type annotations (NestJS providers);
- annotation names (`@Autowired`) and qualified member segments are never
  type uses; parameter names and lowercase identifiers stay untouched;
- all 99 parser tests, strict Clippy, rustdoc, and lossless source
  reconstruction gates.

## 0.2.1 - 2026-07-30

- split every language adapter, shared syntax profile, and token scanner into
  focused domain modules without changing the public API;
- enforce a strict parser architecture with zero runtime cycles, 300-line file
  and 100-line function budgets, no exceptions, and no ambiguous Rust module
  layouts;
- replace internal facade imports with direct fact, syntax, and token-layer
  dependencies;
- modularize the regression and tree-sitter comparison tools while preserving
  their measured methodology and output contracts;
- retain all 97 parser tests, strict Clippy, rustdoc, and lossless source
  reconstruction gates.

## 0.2.0 - 2026-07-29

- add the dependency-free lossless tokenizer and structural adapters used by
  the native Weavatrix engine;
- cover code, schema, configuration, UI, style, document, and shell surfaces
  with exact spans and typed facts;
- benchmark throughput and import agreement against tree-sitter on immutable
  real-repository corpora.
