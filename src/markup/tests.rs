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
