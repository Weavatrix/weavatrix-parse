use super::extract;
use crate::facts::{DeclarationKind, ImportBinding, ReferenceKind};
use crate::syntax::Language;

fn specifiers(source: &str) -> Vec<(String, bool, bool)> {
    extract(source, Language::TypeScript)
        .imports
        .into_iter()
        .map(|import| (import.specifier, import.type_only, import.reexport))
        .collect()
}

mod calls;
mod declarations;
mod imports;
mod templates;
