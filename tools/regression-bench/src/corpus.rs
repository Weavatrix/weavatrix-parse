use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use weavatrix_parse::Language;

pub(super) struct Entry {
    pub(super) identity: String,
    pub(super) source: String,
}

pub(super) fn collect(
    roots: &[PathBuf],
    languages: &[Language],
    limit: usize,
) -> HashMap<Language, Vec<Entry>> {
    let allowed = languages.iter().copied().collect::<HashSet<_>>();
    let mut corpora = HashMap::new();
    let mut totals = HashMap::new();
    for (root_index, root) in roots.iter().enumerate() {
        visit(
            root,
            root,
            root_index,
            &allowed,
            limit,
            &mut corpora,
            &mut totals,
        );
    }
    corpora
}

fn visit(
    root: &Path,
    path: &Path,
    root_index: usize,
    allowed: &HashSet<Language>,
    limit: usize,
    corpora: &mut HashMap<Language, Vec<Entry>>,
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
            visit(root, &path, root_index, allowed, limit, corpora, totals);
            continue;
        }
        let Some(language) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(Language::from_extension)
        else {
            continue;
        };
        if !allowed.contains(&language) || totals.get(&language).copied().unwrap_or(0) >= limit {
            continue;
        }
        if let Ok(source) = fs::read_to_string(&path) {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let identity = format!(
                "{root_index}/{}",
                relative.to_string_lossy().replace('\\', "/")
            );
            *totals.entry(language).or_default() += source.len();
            corpora
                .entry(language)
                .or_default()
                .push(Entry { identity, source });
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
