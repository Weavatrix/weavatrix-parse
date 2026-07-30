use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use weavatrix_parse::Language;

use crate::config::LanguageGrammar;

/// Largest corpus per language, so one huge vendored tree cannot dominate.
pub(crate) const CORPUS_LIMIT: usize = 8 * 1024 * 1024;

/// One corpus file: where it came from, and what it contains.
pub(crate) type Entry = (PathBuf, String);

pub(crate) type Corpora = HashMap<Language, Vec<Entry>>;

/// Reads every selected language in one deterministic filesystem traversal.
pub(crate) fn collect_all(roots: &[String], languages: &[LanguageGrammar]) -> Corpora {
    let allowed = languages
        .iter()
        .map(|(language, _)| *language)
        .collect::<HashSet<_>>();
    let mut corpora = HashMap::new();
    let mut totals = HashMap::new();
    for root in roots {
        visit(Path::new(root), &allowed, &mut corpora, &mut totals);
    }
    corpora
}

fn visit(
    path: &Path,
    allowed: &HashSet<Language>,
    corpora: &mut Corpora,
    totals: &mut HashMap<Language, usize>,
) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if skip_directory(&name) {
            continue;
        }
        if path.is_dir() {
            visit(&path, allowed, corpora, totals);
            continue;
        }
        let Some(language) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(Language::from_extension)
        else {
            continue;
        };
        if !allowed.contains(&language)
            || totals.get(&language).copied().unwrap_or(0) >= CORPUS_LIMIT
        {
            continue;
        }
        if let Ok(source) = fs::read_to_string(&path) {
            *totals.entry(language).or_default() += source.len();
            corpora.entry(language).or_default().push((path, source));
        }
    }
}

fn skip_directory(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "coverage"
                | "out"
                | "__pycache__"
                | "venv"
        )
}
