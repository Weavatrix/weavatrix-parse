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
