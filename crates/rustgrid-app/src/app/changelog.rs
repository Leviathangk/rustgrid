//! The changelog shown at the bottom of 选项 → 关于.
//!
//! Both sources are the repository's curated changelog — `CHANGELOG.md` behind an English UI and
//! `CHANGELOG_zh.md` behind a 简体中文 one — so the panel describes each release in the UI's own
//! language:
//!
//! - This build's own entry, embedded at compile time, so 关于 has something to show before any
//!   check has run and without touching the network.
//! - The newer release's entry, which no build can embed: the release workflow publishes each
//!   version's section as the GitHub release notes (see `.github/workflows/release.yml`), and the
//!   update check already reads that release, so its body carries the notes with it (see
//!   `update.rs`). Those notes hold both languages, split by [`MARKER_EN`] / [`MARKER_ZH`].
//!
//! Only the version the page is about is shown: the newer release while an update is available
//! (what installing would get you), otherwise the running version (what this build brought).

use super::*;

/// The two changelogs, embedded so the panel never needs the network to describe the running build.
const CHANGELOG_EN: &str = include_str!("../../../../CHANGELOG.md");
const CHANGELOG_ZH: &str = include_str!("../../../../CHANGELOG_zh.md");

/// The markers the release workflow writes in front of each language's notes in a release body.
const MARKER_EN: &str = "<!-- rustgrid:en -->";
const MARKER_ZH: &str = "<!-- rustgrid:zh -->";

/// One line of a version's notes, classified so 关于 can style it.
pub(super) enum NoteLine {
    /// A version heading: `0.0.11`, with the release date when the changelog names one.
    Version {
        version: String,
        date: Option<String>,
    },
    /// A group heading (`### Added` / `### 新增`).
    Group(String),
    /// A bullet item, its wrapped continuation lines joined.
    Bullet(String),
    /// A plain paragraph (e.g. the changelog's `First release.`).
    Text(String),
}

/// The notes 关于 shows for `version`: the newer release's when `release_notes` carries them,
/// otherwise this build's own entry from the embedded changelog. Empty only if the changelog has
/// no version heading at all.
pub(super) fn notes(
    language: LanguageSetting,
    version: &str,
    release_notes: Option<&str>,
) -> Vec<NoteLine> {
    if let Some(text) = release_notes.and_then(|body| release_body_section(body, language)) {
        let parsed = parse(&text);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    let embedded = embedded(language);
    version_section(embedded, version)
        .or_else(|| newest_section(embedded))
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// The embedded changelog for one UI language.
fn embedded(language: LanguageSetting) -> &'static str {
    match language {
        LanguageSetting::En => CHANGELOG_EN,
        LanguageSetting::ZhCn => CHANGELOG_ZH,
    }
}

/// The part of a GitHub release body written for one language.
///
/// A body without the markers — a hand-written release, or one published before the changelog was
/// wired into the workflow — is returned whole, so the panel still shows whatever the release said.
pub(super) fn release_body_section(body: &str, language: LanguageSetting) -> Option<String> {
    let marker = match language {
        LanguageSetting::En => MARKER_EN,
        LanguageSetting::ZhCn => MARKER_ZH,
    };
    let Some(start) = body.find(marker) else {
        let body = body.trim();
        return (!body.is_empty()).then(|| body.to_string());
    };
    let rest = &body[start + marker.len()..];
    let end = rest.find("<!-- rustgrid:").unwrap_or(rest.len());
    let section = rest[..end].trim();
    (!section.is_empty()).then(|| section.to_string())
}

/// The changelog block for one version, as written in the file (heading included).
pub(super) fn version_section(text: &str, version: &str) -> Option<String> {
    version_blocks(text)
        .into_iter()
        .find(|block| block_is_version(block, version))
        .map(|block| block.trim_end().to_string())
}

/// The changelog's newest block. The fallback for a build whose `Cargo.toml` was bumped before its
/// changelog entry was written: better the previous entry (which names its own version) than an
/// empty panel.
fn newest_section(text: &str) -> Option<String> {
    version_blocks(text)
        .into_iter()
        .next()
        .map(|block| block.trim_end().to_string())
}

/// The changelog's blocks, each starting at its `## [version]` heading, in file order (newest
/// first). Everything before the first heading — the file's title and preamble — is dropped.
fn version_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let heading = line.trim_end_matches(['\r', '\n']).starts_with("## [");
        if heading {
            if let Some(from) = start {
                blocks.push(&text[from..offset]);
            }
            start = Some(offset);
        }
        offset += line.len();
    }
    if let Some(from) = start {
        blocks.push(&text[from..]);
    }
    blocks
}

/// Whether a block is the entry for `version`. The heading is parsed rather than prefix-matched, so
/// `0.0.1` never claims `0.0.11`'s entry.
fn block_is_version(block: &str, version: &str) -> bool {
    version_heading(block).is_some_and(|(found, _date)| found == version)
}

/// A `## [0.0.11] - 2026-10-15` heading, as its version and (optional) date.
fn version_heading(block: &str) -> Option<(String, Option<String>)> {
    let heading = block.lines().next()?.trim_end().strip_prefix("## ")?;
    let rest = heading.strip_prefix('[')?;
    let (version, rest) = rest.split_once(']')?;
    let date = rest
        .trim()
        .strip_prefix('-')
        .map(str::trim)
        .filter(|date| !date.is_empty())
        .map(str::to_string);
    Some((version.trim().to_string(), date))
}

/// Split a version's notes into renderable lines: headings, group headings, bullets (with their
/// wrapped continuation lines joined) and paragraphs.
pub(super) fn parse(text: &str) -> Vec<NoteLine> {
    let mut lines = Vec::new();
    let mut bullet: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("<!--") {
            push_bullet(&mut lines, &mut bullet);
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            push_bullet(&mut lines, &mut bullet);
            match version_heading(line) {
                Some((version, date)) => lines.push(NoteLine::Version { version, date }),
                None => lines.push(NoteLine::Group(strip_inline(heading))),
            }
        } else if let Some(heading) = line.strip_prefix("### ") {
            push_bullet(&mut lines, &mut bullet);
            lines.push(NoteLine::Group(strip_inline(heading)));
        } else if let Some(item) = line.strip_prefix("- ") {
            push_bullet(&mut lines, &mut bullet);
            bullet = Some(item.trim().to_string());
        } else if let Some(item) = bullet.as_mut() {
            // A wrapped bullet: the changelog keeps its entries inside 100 columns.
            item.push(' ');
            item.push_str(line);
        } else {
            lines.push(NoteLine::Text(strip_inline(line)));
        }
    }
    push_bullet(&mut lines, &mut bullet);
    lines
}

/// Close the bullet being collected, unescaping its inline markdown.
fn push_bullet(lines: &mut Vec<NoteLine>, bullet: &mut Option<String>) {
    if let Some(item) = bullet.take() {
        lines.push(NoteLine::Bullet(strip_inline(&item)));
    }
}

/// Drop the inline markdown a plain-text panel cannot render: `**emphasis**`, `` `code` `` and
/// `[label](target)` keep their text, not their syntax.
fn strip_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['*', '`', '[']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(inner) = tail.strip_prefix('[') {
            match inner.split_once(']') {
                // `[label](url)`: keep the label, skip the target.
                Some((label, after)) if after.starts_with('(') => {
                    out.push_str(label);
                    rest = after.split_once(')').map_or(after, |(_url, rest)| rest);
                }
                _ => {
                    out.push('[');
                    rest = inner;
                }
            }
        } else {
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::super::update::APP_VERSION;
    use super::*;

    const SAMPLE: &str = "\
# Changelog

Intro.

## [0.0.2] - 2026-02-02

### Added

- A **bold** point that wraps onto
  the next line, with `code`.
- Another point.

## [0.0.1] - 2026-01-01

First release.
";

    #[test]
    fn a_version_block_stops_at_the_next_heading() {
        let section = version_section(SAMPLE, "0.0.2").expect("0.0.2 has an entry");
        assert!(section.contains("A **bold** point"));
        assert!(!section.contains("First release"));
    }

    #[test]
    fn a_version_never_claims_its_own_prefix() {
        // `0.0.1` must not match `0.0.11`'s heading, which starts with the same characters.
        let changelog = "## [0.0.11] - 2026-03-03\n\n### Added\n\n- eleven\n";
        assert_eq!(version_section(changelog, "0.0.1"), None);
        assert_eq!(
            version_section(changelog, "0.0.11").as_deref(),
            Some(changelog.trim_end())
        );
    }

    #[test]
    fn notes_are_classified_and_unmarked() {
        let notes = parse(version_section(SAMPLE, "0.0.2").unwrap().as_str());
        assert!(matches!(
            notes.first(),
            Some(NoteLine::Version { version, date })
                if version == "0.0.2" && date.as_deref() == Some("2026-02-02")
        ));
        assert!(matches!(&notes[1], NoteLine::Group(group) if group == "Added"));
        match &notes[2] {
            NoteLine::Bullet(item) => assert_eq!(
                item,
                "A bold point that wraps onto the next line, with code."
            ),
            _ => panic!("expected the wrapped bullet to be joined"),
        }
        assert!(matches!(&notes[3], NoteLine::Bullet(item) if item == "Another point."));
    }

    #[test]
    fn a_release_body_is_split_by_language() {
        let body = format!(
            "{MARKER_EN}\n## [9.9.9]\n\n### Added\n\n- english\n\n{MARKER_ZH}\n## [9.9.9]\n\n### 新增\n\n- 中文\n"
        );
        let english = release_body_section(&body, LanguageSetting::En).unwrap();
        assert!(english.contains("- english") && !english.contains("中文"));
        let chinese = release_body_section(&body, LanguageSetting::ZhCn).unwrap();
        assert!(chinese.contains("- 中文") && !chinese.contains("english"));
    }

    #[test]
    fn a_release_body_without_markers_is_used_as_is() {
        let body = "## What's Changed\n\n* fix a thing\n";
        let section = release_body_section(body, LanguageSetting::En).unwrap();
        assert_eq!(section, body.trim());
    }

    #[test]
    fn the_release_notes_win_over_the_embedded_entry() {
        // The exact shape the release job publishes: each changelog's section for the tagged
        // version, behind its language marker (see `.github/workflows/release.yml`). 关于 must read
        // it back as the notes of the newer release — the running build cannot embed them — in the
        // UI's own language.
        let english = version_section(CHANGELOG_EN, "0.0.11").unwrap();
        let chinese = version_section(CHANGELOG_ZH, "0.0.11").unwrap();
        let body = format!("{MARKER_EN}\n{english}\n\n{MARKER_ZH}\n{chinese}\n");
        let english_notes = notes(LanguageSetting::En, "0.0.1", Some(&body));
        assert!(matches!(
            english_notes.first(),
            Some(NoteLine::Version { version, .. }) if version == "0.0.11"
        ));
        let chinese_notes = notes(LanguageSetting::ZhCn, "0.0.1", Some(&body));
        assert!(matches!(
            chinese_notes.first(),
            Some(NoteLine::Version { version, .. }) if version == "0.0.11"
        ));
        assert!(matches!(&chinese_notes[1], NoteLine::Group(group) if group == "新增"));
    }

    #[test]
    fn the_embedded_changelog_documents_the_running_version() {
        // Guards the release rule: bumping the workspace version without writing the entry in both
        // changelogs leaves 关于 with nothing to show.
        for (language, name) in [
            (LanguageSetting::En, "CHANGELOG.md"),
            (LanguageSetting::ZhCn, "CHANGELOG_zh.md"),
        ] {
            let notes = notes(language, APP_VERSION, None);
            assert!(
                matches!(
                    notes.first(),
                    Some(NoteLine::Version { version, .. }) if version == APP_VERSION
                ),
                "{name} has no entry for version {APP_VERSION}"
            );
        }
    }
}
