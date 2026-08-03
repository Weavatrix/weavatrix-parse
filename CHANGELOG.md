# Changelog

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
