use super::*;

#[test]
fn a_comment_never_declares_anything_in_any_brace_language() {
    for (source, language) in [
        (
            "// pub fn ghost() {}\n/* struct Ghost; */\npub fn real() {}\n",
            Language::Rust,
        ),
        ("// func Ghost() {}\nfunc Real() {}\n", Language::Go),
    ] {
        let items = declared(source, language);
        assert_eq!(
            items.len(),
            1,
            "only the real declaration counts: {items:?}"
        );
    }
}
