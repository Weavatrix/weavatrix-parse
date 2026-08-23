use super::extract;
use crate::facts::{DeclarationKind, ImportBinding, ReferenceKind};
use crate::syntax::Language;

fn declared(source: &str, language: Language) -> Vec<(String, DeclarationKind, Option<String>)> {
    extract(source, language)
        .declarations
        .into_iter()
        .map(|item| (item.name, item.kind, item.owner))
        .collect()
}

mod go;
mod managed;
mod native;
mod robustness;
mod rust;
mod swift;
