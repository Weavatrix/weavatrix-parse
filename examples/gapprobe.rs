//! Temporary probe: what do the current extractors actually see?
use weavatrix_parse::{Language, extract, tokenize_lite, TokenKind};

fn show(label: &str, source: &str, language: Language) {
    let facts = extract(source, language);
    println!("=== {label} ({}) ===", language.as_str());
    println!(
        "  imports: {:?}",
        facts.imports.iter().map(|i| i.specifier.as_str()).collect::<Vec<_>>()
    );
    println!(
        "  decls:   {:?}",
        facts
            .declarations
            .iter()
            .map(|d| (d.name.as_str(), d.kind))
            .collect::<Vec<_>>()
    );
    println!(
        "  refs:    {:?}",
        facts
            .references
            .iter()
            .map(|r| (r.name.as_str(), r.kind))
            .collect::<Vec<_>>()
    );
}

fn main() {
    // 1. Vue SFC — extension maps to Html today.
    let vue = "<template>\n  <div class=\"card\"><Child :n=\"count\"/></div>\n</template>\n\
<script setup>\nimport Child from './Child.vue';\nimport { useStore } from '../store';\n\
const count = ref(0);\nfunction bump() { useStore().inc(); }\n</script>\n\
<style scoped>\n.card { color: red; }\n</style>\n";
    show("Vue SFC", vue, Language::Html);

    // 2. Svelte
    let svelte = "<script>\n  import Nav from './Nav.svelte';\n  export let title;\n</script>\n\
<h1 class=\"t\">{title}</h1>\n<style>.t { font-size: 2rem; }</style>\n";
    show("Svelte SFC", svelte, Language::Html);

    // 3. Plain HTML with inline script (no src)
    let html = "<html><body>\n<script>\nimport { boot } from './boot.js';\nboot();\n</script>\n\
</body></html>\n";
    show("HTML inline script", html, Language::Html);

    // 4. Bash — tokenized, no structural extraction
    let bash = "#!/usr/bin/env bash\nsource ./lib/common.sh\n. \"$DIR/env.sh\"\n\
deploy() {\n  build_image\n}\ndeploy\n";
    show("Bash", bash, Language::Bash);
    let bt = tokenize_lite(bash, Language::Bash);
    println!("  bash token kinds: {:?}", bt.iter().map(|t| t.kind).take(12).collect::<Vec<_>>());

    // 5. Bash heredoc — does it mis-scan?
    let heredoc = "cat <<'EOF' > out.txt\nit's a quote \" here\nEOF\nsource ./after.sh\n";
    let ht = tokenize_lite(heredoc, Language::Bash);
    println!(
        "=== Bash heredoc ===\n  unterminated/string tokens: {:?}",
        ht.iter()
            .filter(|t| matches!(t.kind, TokenKind::String | TokenKind::Unterminated))
            .map(|t| (t.kind, t.text(heredoc)))
            .collect::<Vec<_>>()
    );

    // 6. YAML — tokenized, no structural extraction
    let yaml = "jobs:\n  build:\n    uses: ./.github/workflows/ci.yml\n    steps:\n      - uses: actions/checkout@v4\n";
    show("YAML", yaml, Language::Yaml);

    // 7. C free function — braced_members claim
    let c = "#include \"util.h\"\nint add(int a, int b) { return a + b; }\n\
struct Point { int x; };\nvoid run(void) { add(1,2); }\n";
    show("C", c, Language::C);

    // 8. Does the Html syntax tokenize an embedded <script> body sanely?
    let toks = tokenize_lite(vue, Language::Html);
    println!(
        "=== Vue tokenized as HTML, first 40 ===\n  {:?}",
        toks.iter().take(40).map(|t| (t.kind, t.text(vue))).collect::<Vec<_>>()
    );
}
