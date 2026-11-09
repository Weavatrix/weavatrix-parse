export type Language = 'javascript' | 'typescript' | 'graphql' | 'protobuf' | 'rust' | 'python' | 'go' | 'java' | 'csharp' | 'c' | 'cpp' | 'sql' | 'solidity' | 'swift' | 'terraform' | 'html' | 'xml' | 'markdown' | 'mdx' | 'rst' | 'asciidoc' | 'css' | 'scss' | 'bash' | 'yaml'

export interface Span { start: number; end: number; line: number; column: number; endLine: number; endColumn: number }
export interface Declaration { name: string; kind: string; span: Span; extent: Span; owner?: string; exported: boolean; testOnly: boolean }
export interface ImportBinding { imported: string; local: string }
export interface ImportFact { specifier: string; span: Span; typeOnly: boolean; reexport: boolean; names: string[]; bindings: ImportBinding[] }
export interface Reference { name: string; kind: string; receiver?: string; span: Span; owner?: string; stringArguments: string[]; nameArguments: string[] }
export interface Contract { name: string; kind: { type: string; [key: string]: unknown }; span: Span; owner?: string }
export interface ParseDiagnostic { code: string; message: string; span: Span }
export interface Facts { declarations: Declaration[]; imports: ImportFact[]; references: Reference[]; contracts: Contract[]; diagnostics: ParseDiagnostic[] }
export interface Token { kind: string; start: number; end: number; line: number; column: number; text: string }

export declare function extract(source: string, language: Language | string): Facts
export declare function extractPath(path: string, source: string): Facts | undefined
export declare function tokenize(source: string, language: Language | string, options?: { mode?: 'lossless' | 'lite' }): Token[]
export declare function supportedLanguages(): Language[]
