//! Structural extraction for Markdown, MDX, `reStructuredText` and `AsciiDoc`.
//!
//! Prose has no token structure worth the name: a `"` is a quotation mark, not
//! a literal, and `//` is part of a URL. So this reads lines directly rather
//! than through the tokenizer, which is the honest model for the format even
//! though it is the opposite of what every other extractor here does.
//!
//! Two facts are worth having. A heading is a named anchor other documents
//! link to, and nesting one heading under another is the document's own table
//! of contents. A link to a path in the repository is a dependency exactly as
//! an import is - which is what turns a documentation tree into part of the
//! graph rather than a pile of files beside it.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Span};
use crate::syntax::Language;

/// Extracts structural facts from one document.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let mut state = Extractor {
        facts: Facts::default(),
        headings: Vec::new(),
        offset: 0,
    };
    state.run(source, language);
    state.facts
}

/// A heading whose section later headings may nest inside.
struct Heading {
    name: String,
    level: usize,
}

struct Extractor {
    facts: Facts,
    headings: Vec<Heading>,
    offset: usize,
}

impl Extractor {
    fn run(&mut self, source: &str, language: Language) {
        let lines = source.lines().collect::<Vec<_>>();
        let mut fenced = false;
        let mut script = String::new();
        for (number, line) in lines.iter().enumerate() {
            let start = self.offset;
            self.offset += line.len() + 1;
            let trimmed = line.trim();
            // A fence hides everything until it closes: code inside a block is
            // an example, and reading its links would invent dependencies.
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let span = Span {
                start,
                end: start + line.len(),
                line: u32::try_from(number + 1).unwrap_or(u32::MAX),
                column: 1,
                end_line: u32::try_from(number + 1).unwrap_or(u32::MAX),
                end_column: u32::try_from(line.len() + 1).unwrap_or(u32::MAX),
            };
            // MDX holds real JavaScript imports, which the script extractor
            // already reads correctly; gathering them keeps that one rule.
            if language == Language::Mdx
                && (trimmed.starts_with("import ") || trimmed.starts_with("export "))
            {
                script.push_str(line);
                script.push('\n');
                continue;
            }
            self.heading(trimmed, lines.get(number + 1).copied(), language, span);
            self.targets(line, language, span);
        }
        if !script.is_empty() {
            let inner = crate::script::extract(&script, Language::TypeScript);
            self.facts.imports.extend(inner.imports);
        }
    }

    /// Records a heading and nests it under the last shallower one.
    fn heading(&mut self, line: &str, next: Option<&str>, language: Language, span: Span) {
        let Some((level, name)) = read_heading(line, next, language) else {
            return;
        };
        while self
            .headings
            .last()
            .is_some_and(|heading| heading.level >= level)
        {
            self.headings.pop();
        }
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Heading,
            span,
            owner: self.headings.last().map(|heading| heading.name.clone()),
            // Every heading in a document is reachable by anchor.
            exported: true,
        });
        self.headings.push(Heading { name, level });
    }

    /// Records every path this line points at.
    fn targets(&mut self, line: &str, language: Language, span: Span) {
        for target in link_targets(line, language) {
            if !is_repository_path(&target) {
                continue;
            }
            self.facts.imports.push(Import {
                specifier: target,
                span,
                type_only: false,
                reexport: false,
                names: Vec::new(),
            });
        }
    }
}

/// The heading level and text on this line, if it is one.
fn read_heading(line: &str, next: Option<&str>, language: Language) -> Option<(usize, String)> {
    let marker = match language {
        // AsciiDoc writes `== Title`; Markdown writes `## Title`.
        Language::AsciiDoc => '=',
        Language::ReStructuredText => {
            // A heading is text with a rule of repeated punctuation beneath it,
            // and the character used sets the level by order of first use.
            let under = next?.trim();
            if line.is_empty() || under.len() < line.len() {
                return None;
            }
            let mark = under.chars().next()?;
            if !"=-`:'\"~^_*+#<>".contains(mark) || under.chars().any(|other| other != mark) {
                return None;
            }
            let level = "=-`:'\"~^_*+#<>".find(mark).unwrap_or(0) + 1;
            return Some((level, line.to_owned()));
        }
        _ => '#',
    };
    let level = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = line[level..].trim();
    // `#tag` and `=value` are not headings; a heading separates its marker.
    if rest.is_empty() || !line[level..].starts_with(' ') {
        return None;
    }
    Some((level, rest.to_owned()))
}

/// Every path this line points at, in whichever way the format writes one.
fn link_targets(line: &str, language: Language) -> Vec<String> {
    let mut found = Vec::new();
    match language {
        Language::ReStructuredText => {
            // `.. include:: path`, `.. image:: path`, `.. figure:: path`.
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("..")
                && let Some((directive, argument)) = rest.trim_start().split_once("::")
                && matches!(
                    directive.trim(),
                    "include" | "image" | "figure" | "literalinclude"
                )
            {
                found.push(argument.trim().to_owned());
            }
        }
        Language::AsciiDoc => {
            // `include::path[]` and `image::path[opts]`.
            for directive in ["include::", "image::"] {
                let mut rest = line;
                while let Some(at) = rest.find(directive) {
                    let after = &rest[at + directive.len()..];
                    if let Some(end) = after.find('[') {
                        found.push(after[..end].trim().to_owned());
                    }
                    rest = after;
                }
            }
        }
        _ => {
            // `[text](./path)`, `![alt](./image.png)` and the reference form
            // `[id]: ./path`.
            let bytes = line.as_bytes();
            let mut at = 0;
            while at < bytes.len() {
                if bytes[at] == b']'
                    && let Some(rest) = line.get(at + 1..)
                {
                    if let Some(inner) = rest.strip_prefix('(')
                        && let Some(end) = inner.find(')')
                    {
                        // A title may follow the path inside the parentheses.
                        let target = inner[..end].split_whitespace().next().unwrap_or("");
                        found.push(target.to_owned());
                    } else if let Some(inner) = rest.strip_prefix(": ") {
                        found.push(inner.trim().to_owned());
                    }
                }
                at += 1;
            }
        }
    }
    found
}

/// Whether a link target names a file in this repository rather than the web.
fn is_repository_path(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with('#')
        && !target.starts_with("//")
        && !target.contains("://")
        && !target.starts_with("mailto:")
        && !target.starts_with("tel:")
        && !target.starts_with('<')
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::DeclarationKind;
    use crate::syntax::Language;

    fn imports(source: &str, language: Language) -> Vec<String> {
        extract(source, language)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect()
    }

    #[test]
    fn headings_nest_into_the_documents_own_table_of_contents() {
        let source = "# Guide\n\
             \n\
             ## Install\n\
             \n\
             ### From source\n\
             \n\
             ## Usage\n\
             \n\
             Not a heading: #tag and # \n";
        let declared = extract(source, Language::Markdown)
            .declarations
            .into_iter()
            .map(|item| (item.name, item.owner))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            [
                ("Guide".to_owned(), None),
                ("Install".to_owned(), Some("Guide".to_owned())),
                ("From source".to_owned(), Some("Install".to_owned())),
                ("Usage".to_owned(), Some("Guide".to_owned())),
            ],
            "a heading nests under the last shallower one"
        );
    }

    #[test]
    fn a_link_to_the_repository_is_a_dependency_and_a_url_is_not() {
        let source = "See [the guide](./docs/guide.md) and [the API](../api/index.md \"title\").\n\
             ![diagram](assets/flow.png)\n\
             [home]: https://example.com\n\
             [local]: ./other.md\n\
             Jump to [section](#usage) or mail [us](mailto:x@y.z).\n";
        assert_eq!(
            imports(source, Language::Markdown),
            [
                "./docs/guide.md",
                "../api/index.md",
                "assets/flow.png",
                "./other.md",
            ],
            "an anchor, a URL and a mail address are not files"
        );
    }

    #[test]
    fn a_fenced_block_is_an_example_rather_than_a_dependency() {
        let source = "Real [link](./real.md).\n\
             \n\
             ```markdown\n\
             [ghost](./ghost.md)\n\
             ```\n\
             \n\
             ~~~\n\
             [also-ghost](./also.md)\n\
             ~~~\n";
        assert_eq!(imports(source, Language::Markdown), ["./real.md"]);
    }

    #[test]
    fn mdx_keeps_real_javascript_imports() {
        let source = "import Chart from './Chart.jsx';\n\
             import { note } from '../notes';\n\
             \n\
             # Report\n\
             \n\
             See [details](./details.mdx).\n\
             \n\
             <Chart data={note} />\n";
        assert_eq!(
            imports(source, Language::Mdx),
            ["./details.mdx", "./Chart.jsx", "../notes"],
            "a component import is a dependency, not prose"
        );
    }

    #[test]
    fn restructured_text_headings_and_includes() {
        let source = "Guide\n\
             =====\n\
             \n\
             .. include:: ../shared/intro.rst\n\
             \n\
             Install\n\
             -------\n\
             \n\
             .. image:: assets/logo.png\n";
        let facts = extract(source, Language::ReStructuredText);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|item| (item.name.as_str(), item.owner.as_deref()))
                .collect::<Vec<_>>(),
            [("Guide", None), ("Install", Some("Guide"))],
            "the underline character sets the level"
        );
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["../shared/intro.rst", "assets/logo.png"]
        );
    }

    #[test]
    fn asciidoc_headings_and_includes() {
        let source = "= Guide\n\
             \n\
             include::shared/intro.adoc[]\n\
             \n\
             == Install\n\
             \n\
             image::assets/logo.png[width=200]\n";
        let facts = extract(source, Language::AsciiDoc);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|item| (item.name.as_str(), item.owner.as_deref()))
                .collect::<Vec<_>>(),
            [("Guide", None), ("Install", Some("Guide"))]
        );
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["shared/intro.adoc", "assets/logo.png"]
        );
    }

    #[test]
    fn every_heading_carries_the_kind_the_graph_stores_it_under() {
        let facts = extract("# Only\n", Language::Markdown);
        assert_eq!(facts.declarations[0].kind, DeclarationKind::Heading);
        assert_eq!(facts.declarations[0].span.line, 1);
    }
}
