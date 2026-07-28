//! Structural extraction for HTML and single-file components.
//!
//! HTML contributes two kinds of edge. A `<link href>`, `<script src>` or
//! `<img src>` names another file, which is an ordinary dependency. A `class`
//! or `id` attribute names a CSS selector, which resolves to whichever
//! stylesheet declares it - an edge that exists only once both sides are
//! parsed, and the reason a document and its stylesheet belong in one graph.
//!
//! Attributes are read from the token stream, so a tag written inside a
//! comment contributes nothing, and a `<` inside an attribute value does not
//! open a tag.

use crate::facts::{Facts, Import, Span};
use crate::style::selector_use;
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one document.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        language,
        facts: Facts::default(),
        tag: String::new(),
        text_start: None,
    };
    state.run();
    state.facts
}

/// Elements whose text is a dependency rather than prose.
///
/// XML project files name their dependencies in element content rather than in
/// attributes: a Maven module lists `<module>ui</module>`, and an artifact is
/// split across `<groupId>` and `<artifactId>`.
fn names_a_file_in_text(tag: &str) -> bool {
    matches!(tag, "module" | "include" | "xi:include" | "systemid")
}

/// Which attribute names a file, given the tag it is written on.
///
/// `href` on an anchor is a link to a page rather than a dependency of this
/// one, so the tag decides, not the attribute alone.
fn names_a_file(language: Language, tag: &str, attribute: &str) -> bool {
    if language == Language::Xml {
        // A project file points at another project or package by attribute:
        // `<ProjectReference Include="../Lib/Lib.csproj">`, `<xi:include
        // href="shared.xml">`, `<xsd:import schemaLocation="types.xsd">`.
        return matches!(
            attribute,
            "include" | "href" | "src" | "schemalocation" | "location" | "file" | "path"
        );
    }
    match attribute {
        "href" => matches!(tag, "link" | "use"),
        "src" => matches!(
            tag,
            "script" | "img" | "iframe" | "audio" | "video" | "source" | "embed" | "track"
        ),
        "srcset" | "data-src" => true,
        _ => false,
    }
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    language: Language,
    facts: Facts,
    /// The tag whose attributes are being read.
    tag: String,
    /// Where the text of an element whose content names a file begins.
    text_start: Option<(usize, String)>,
}

impl Extractor<'_, '_> {
    fn run(&mut self) {
        let mut index = 0;
        while index < self.tokens.len() {
            index = self.step(index);
        }
    }

    fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let last_index = self.tokens.len().saturating_sub(1);
        let first = &self.tokens[start.min(last_index)];
        let last = &self.tokens[end.min(last_index)];
        Span {
            start: first.start,
            end: last.end,
            line: first.line,
            column: first.column,
            end_line: last.line,
            end_column: last.column,
        }
    }

    fn step(&mut self, index: usize) -> usize {
        // `<tag` opens an element and names the tag the following attributes
        // belong to; `</` and `>` end one.
        if self.punct(index, "<") {
            // An element whose content names a file ends its text here.
            self.close_text(index);
            if self.kind(index + 1) == Some(TokenKind::Identifier) {
                self.tag = self.text(index + 1).to_ascii_lowercase();
                return index + 2;
            }
            self.tag.clear();
            return index + 1;
        }
        if self.punct(index, ">") {
            // A script or style element holds another language, and its
            // contents are the whole point of a single-file component: a Vue
            // or Svelte file keeps its imports there and nowhere else.
            if matches!(self.tag.as_str(), "script" | "style") {
                let embedded = self.tag.clone();
                self.tag.clear();
                return self.embedded(index, &embedded);
            }
            if self.language == Language::Xml && names_a_file_in_text(&self.tag) {
                let tag = self.tag.clone();
                self.text_start = self.tokens.get(index + 1).map(|token| (token.start, tag));
            }
            self.tag.clear();
            return index + 1;
        }
        if self.tag.is_empty() {
            return index + 1;
        }
        self.attribute(index)
    }

    /// Records the text of an element whose content is a path.
    fn close_text(&mut self, index: usize) {
        let Some((start, _)) = self.text_start.take() else {
            return;
        };
        let Some(end) = self.tokens.get(index).map(|token| token.start) else {
            return;
        };
        if end <= start {
            return;
        }
        let text = self.source[start..end].trim();
        // A path has no spaces in it; prose does.
        if text.is_empty() || text.contains(char::is_whitespace) {
            return;
        }
        self.facts.imports.push(Import {
            specifier: text.to_owned(),
            span: self.span(index, index),
            type_only: false,
            reexport: false,
        });
    }

    /// Extracts the body of a `<script>` or `<style>` element with the
    /// extractor for the language it holds, and moves the facts into this
    /// file's coordinates.
    fn embedded(&mut self, close_of_open_tag: usize, tag: &str) -> usize {
        let start_token = close_of_open_tag + 1;
        let Some(start) = self.tokens.get(start_token).map(|token| token.start) else {
            return close_of_open_tag + 1;
        };
        // The body runs to the `<` that opens the closing tag.
        let mut end_token = start_token;
        while end_token < self.tokens.len() {
            if self.punct(end_token, "<")
                && self.punct(end_token + 1, "/")
                && self.text(end_token + 2).eq_ignore_ascii_case(tag)
            {
                break;
            }
            end_token += 1;
        }
        let end = self
            .tokens
            .get(end_token)
            .map_or(self.source.len(), |token| token.start);
        if end <= start {
            return end_token.max(close_of_open_tag + 1);
        }
        let body = &self.source[start..end];
        let language = if tag == "style" {
            // Everything a component writes in a style block is at least SCSS,
            // and reading plain CSS with SCSS rules costs only accepting `//`.
            Language::Scss
        } else {
            Language::TypeScript
        };
        let inner = if tag == "style" {
            crate::style::extract(body, language)
        } else {
            crate::script::extract(body, language)
        };
        let line = self.tokens[start_token].line;
        let column = self.tokens[start_token].column;
        self.absorb(inner, start, line, column);
        end_token
    }

    /// Moves facts from a fragment's coordinates into the document's.
    fn absorb(&mut self, mut inner: Facts, offset: usize, line: u32, column: u32) {
        let shift = |span: &mut Span| {
            // Only the fragment's first line shares a line with the document,
            // so only it needs the column moved.
            if span.line == 1 {
                span.column += column - 1;
            }
            if span.end_line == 1 {
                span.end_column += column - 1;
            }
            span.start += offset;
            span.end += offset;
            span.line += line - 1;
            span.end_line += line - 1;
        };
        for item in &mut inner.declarations {
            shift(&mut item.span);
        }
        for item in &mut inner.imports {
            shift(&mut item.span);
        }
        for item in &mut inner.references {
            shift(&mut item.span);
        }
        self.facts.declarations.append(&mut inner.declarations);
        self.facts.imports.append(&mut inner.imports);
        self.facts.references.append(&mut inner.references);
    }

    /// `name="value"`, `name='value'` or `name=value`.
    fn attribute(&mut self, index: usize) -> usize {
        if self.kind(index) != Some(TokenKind::Identifier) || !self.punct(index + 1, "=") {
            return index + 1;
        }
        // A namespace prefix does not change what the attribute means:
        // `xlink:href` names a file exactly as `href` does.
        let written = self.text(index).to_ascii_lowercase();
        let name = written
            .rsplit_once(':')
            .map_or(written.as_str(), |(_, local)| local)
            .to_owned();
        let value_index = index + 2;
        let raw = self.text(value_index);
        // Owned before any push, because the borrow of the token text and the
        // borrow of the fact list are both of `self`.
        let value = match self.kind(value_index) {
            Some(TokenKind::String) => raw.trim_matches(['"', '\'']).to_owned(),
            Some(TokenKind::Identifier | TokenKind::Number) => raw.to_owned(),
            _ => return index + 2,
        };
        if value.is_empty() {
            return value_index + 1;
        }
        let span = self.span(index, value_index);
        match name.as_str() {
            "class" => {
                for class in value.split_whitespace() {
                    selector_use(&mut self.facts, format!(".{class}"), span);
                }
            }
            "id" => selector_use(&mut self.facts, format!("#{value}"), span),
            _ if names_a_file(self.language, &self.tag, &name) => {
                // A srcset lists several candidates with descriptors.
                for candidate in value.split(',') {
                    let path = candidate.split_whitespace().next().unwrap_or("");
                    // A data URI or an external URL is not a file in this tree.
                    if path.is_empty() || path.contains(':') || path.starts_with("//") {
                        continue;
                    }
                    self.facts.imports.push(Import {
                        specifier: path.to_owned(),
                        span,
                        type_only: false,
                        reexport: false,
                    });
                }
            }
            _ => {}
        }
        value_index + 1
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::ReferenceKind;
    use crate::syntax::Language;

    fn imports(source: &str) -> Vec<String> {
        extract(source, Language::Html)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect()
    }

    fn uses(source: &str) -> Vec<String> {
        extract(source, Language::Html)
            .references
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Uses)
            .map(|reference| reference.name)
            .collect()
    }

    #[test]
    fn a_document_depends_on_the_files_it_pulls_in() {
        let source = "<html>\n\
             <head>\n\
             <link rel=\"stylesheet\" href=\"./styles/app.css\">\n\
             <script src=\"/js/main.js\"></script>\n\
             </head>\n\
             <body>\n\
             <img src=\"assets/logo.png\" alt=\"logo\">\n\
             <a href=\"/about\">about</a>\n\
             <script src=\"https://cdn.example.com/x.js\"></script>\n\
             </body>\n\
             </html>\n";
        assert_eq!(
            imports(source),
            ["./styles/app.css", "/js/main.js", "assets/logo.png"],
            "an anchor is navigation and a CDN script is not a file in this tree"
        );
    }

    #[test]
    fn class_and_id_attributes_use_the_selectors_a_stylesheet_declares() {
        let source = "<div class=\"panel panel--wide\" id=\"root\">\n\
             <span class=\"badge\">x</span>\n\
             </div>\n";
        assert_eq!(
            uses(source),
            [".panel", ".panel--wide", "#root", ".badge"],
            "a class attribute names one selector per word"
        );
    }

    #[test]
    fn a_single_file_component_keeps_its_imports_in_its_script_block() {
        // Claiming `.vue` and `.svelte` while reading only tag attributes was
        // worse than not claiming them: the file became a graph node with no
        // dependencies at all, which reads as "this component imports
        // nothing" rather than as "unsupported".
        let source = "<template>\n\
             \x20 <div class=\"card\"><Child /></div>\n\
             </template>\n\
             <script>\n\
             import Child from './Child.vue';\n\
             import { useStore } from '../store';\n\
             function mounted() { useStore(); }\n\
             </script>\n\
             <style scoped>\n\
             .card { color: red; }\n\
             </style>\n";
        let facts = extract(source, Language::Html);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["./Child.vue", "../store"]
        );
        let mounted = facts
            .declarations
            .iter()
            .find(|item| item.name == "mounted")
            .expect("the script block declares a function");
        assert_eq!(
            mounted.span.line, 7,
            "a fact from an embedded block must carry the document's line"
        );
        assert!(
            facts.declarations.iter().any(|item| item.name == ".card"),
            "the style block declares a selector"
        );
        assert!(
            facts
                .references
                .iter()
                .any(|reference| reference.name == ".card"),
            "and the template uses it"
        );
    }

    #[test]
    fn a_project_file_names_the_projects_and_packages_it_references() {
        let source = "<Project Sdk=\"Microsoft.NET.Sdk\">\n\
             \x20 <ItemGroup>\n\
             \x20   <ProjectReference Include=\"../Core/Core.csproj\" />\n\
             \x20   <PackageReference Include=\"Serilog\" Version=\"3.1.0\" />\n\
             \x20 </ItemGroup>\n\
             \x20 <Import Project=\"build/common.props\" />\n\
             </Project>\n";
        assert_eq!(
            extract(source, Language::Xml)
                .imports
                .into_iter()
                .map(|import| import.specifier)
                .collect::<Vec<_>>(),
            ["../Core/Core.csproj", "Serilog"],
            "an Include names a dependency whether it is a path or a package"
        );
    }

    #[test]
    fn a_maven_module_is_named_by_its_element_text() {
        let source = "<project>\n\
             \x20 <modules>\n\
             \x20   <module>ui</module>\n\
             \x20   <module>service</module>\n\
             \x20 </modules>\n\
             \x20 <name>My Project Name</name>\n\
             </project>\n";
        assert_eq!(
            extract(source, Language::Xml)
                .imports
                .into_iter()
                .map(|import| import.specifier)
                .collect::<Vec<_>>(),
            ["ui", "service"],
            "prose with spaces in it is not a path"
        );
    }

    #[test]
    fn a_tag_written_inside_a_comment_contributes_nothing() {
        let source = "<!-- <script src=\"./ghost.js\"></script> -->\n\
             <script src=\"./real.js\"></script>\n";
        assert_eq!(imports(source), ["./real.js"]);
    }

    #[test]
    fn an_angle_bracket_inside_a_value_does_not_open_a_tag() {
        let source = "<div data-tpl=\"<b>bold</b>\" class=\"kept\"></div>\n";
        assert_eq!(
            uses(source),
            [".kept"],
            "the attribute value is one string token, not markup"
        );
    }

    #[test]
    fn a_srcset_names_every_candidate_without_its_descriptor() {
        assert_eq!(
            imports("<img srcset=\"small.png 480w, large.png 1080w\" src=\"fallback.png\">\n"),
            ["small.png", "large.png", "fallback.png"]
        );
    }

    #[test]
    fn hyphenated_and_namespaced_attributes_are_read_as_one_name() {
        let source = "<use xlink:href=\"./sprite.svg#icon\"></use>\n\
             <div data-count=\"3\" aria-label=\"n\" class=\"c\"></div>\n";
        assert_eq!(imports(source), ["./sprite.svg#icon"]);
        assert_eq!(uses(source), [".c"]);
    }
}
