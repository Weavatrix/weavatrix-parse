use super::Language;

/// The heading level and text on this line, if it is one.
pub(super) fn read_heading(
    line: &str,
    next: Option<&str>,
    language: Language,
) -> Option<(usize, String)> {
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
pub(super) fn link_targets(line: &str, language: Language) -> Vec<String> {
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
pub(super) fn is_repository_path(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with('#')
        && !target.starts_with("//")
        && !target.contains("://")
        && !target.starts_with("mailto:")
        && !target.starts_with("tel:")
        && !target.starts_with('<')
}
