//! Line-level edits for the query editor (comment toggle, duplicate, swap, move), as pure functions
//! testable without a window.
//!
//! Each returns a [`LineEdit`]: the byte range to replace, its replacement, and the selection to
//! restore afterwards. The editor applies it as one undoable replacement.

use std::ops::Range;

/// One edit: replace `range` with `text`, then select `selection` (byte offsets in the new text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LineEdit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Range<usize>,
}

/// The byte range of the whole lines the selection touches (without the final line break). A
/// selection ending at the very start of a line does not include that line.
fn line_block(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = text[..selection.start].rfind('\n').map_or(0, |at| at + 1);
    let mut last = selection.end;
    if selection.end > selection.start && text[..selection.end].ends_with('\n') {
        last -= 1;
    }
    let end = text[last..].find('\n').map_or(text.len(), |at| last + at);
    start..end.max(start)
}

/// The comment marker a line starts with (after indentation), if any: `//` (RustGrid's default,
/// as in Navicat), `--`, and `#` on MySQL / MariaDB (`hash_comments`). The marker's following
/// space, if any, is part of it.
fn comment_marker(line: &str, hash_comments: bool) -> Option<&'static str> {
    let trimmed = line.trim_start();
    let markers: &[&'static str] = if hash_comments {
        &["// ", "//", "-- ", "--", "# ", "#"]
    } else {
        &["// ", "//", "-- ", "--"]
    };
    markers
        .iter()
        .copied()
        .find(|marker| trimmed.starts_with(marker))
}

/// Toggle comments (Ctrl+K / Ctrl+/), Navicat-style:
///
/// - nothing selected, or a selection reaching across lines: toggle a line comment on every
///   touched line, as Navicat writes it (`# ` on MySQL / MariaDB, `-- ` elsewhere). If every
///   non-blank line is already commented (`--`, `//`, or `#` on MySQL) they are uncommented,
///   otherwise every non-blank line gets the marker at the block's common indentation;
/// - a selection inside one line: a line comment would run to the end of the line, so the
///   selected part is wrapped in `/* … */` (or unwrapped when it already is a block comment).
pub(super) fn toggle_comment(text: &str, selection: Range<usize>, hash_comments: bool) -> LineEdit {
    let selected = &text[selection.clone()];
    let partial_line = !selected.is_empty()
        && !selected.contains('\n')
        && line_block(text, &selection) != selection;
    if partial_line {
        let trimmed = selected.trim();
        let replacement = match trimmed
            .strip_prefix("/*")
            .and_then(|inner| inner.strip_suffix("*/"))
        {
            Some(inner) => inner.trim().to_string(),
            None => format!("/* {selected} */"),
        };
        let end = selection.start + replacement.len();
        return LineEdit {
            range: selection.clone(),
            selection: selection.start..end,
            text: replacement,
        };
    }

    let block = line_block(text, &selection);
    let lines: Vec<&str> = text[block.clone()].split('\n').collect();
    let content: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let uncomment = !content.is_empty()
        && content
            .iter()
            .all(|line| comment_marker(line, hash_comments).is_some());
    let indent = content
        .iter()
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    // Navicat's markers: `#` on MySQL / MariaDB, `--` (standard SQL) elsewhere.
    let marker = if hash_comments { "# " } else { "-- " };

    let mut out = Vec::with_capacity(lines.len());
    for line in &lines {
        if line.trim().is_empty() {
            out.push(line.to_string());
        } else if uncomment {
            let marker = comment_marker(line, hash_comments).unwrap_or("");
            let at = line.len() - line.trim_start().len();
            out.push(format!("{}{}", &line[..at], &line[at + marker.len()..]));
        } else {
            out.push(format!("{}{marker}{}", &line[..indent], &line[indent..]));
        }
    }
    let replacement = out.join("\n");
    let new_end = block.start + replacement.len();
    LineEdit {
        range: block.clone(),
        selection: block.start..new_end,
        text: replacement,
    }
}

/// Duplicate the selection after itself (selecting the copy), or with nothing selected duplicate
/// the caret's line below it, keeping the caret's column on the copy.
pub(super) fn duplicate(text: &str, selection: Range<usize>) -> LineEdit {
    if selection.start != selection.end {
        let copy = &text[selection.clone()];
        let at = selection.end;
        return LineEdit {
            range: at..at,
            text: copy.to_string(),
            selection: at..at + copy.len(),
        };
    }
    let block = line_block(text, &selection);
    let line = &text[block.clone()];
    let column = selection.start - block.start;
    let caret = block.end + 1 + column;
    LineEdit {
        range: block.end..block.end,
        text: format!("\n{line}"),
        selection: caret..caret,
    }
}

/// Move the selected lines one line up (`up`) or down, keeping them selected. `None` at the edge.
pub(super) fn move_lines(text: &str, selection: Range<usize>, up: bool) -> Option<LineEdit> {
    let block = line_block(text, &selection);
    let moved = &text[block.clone()];
    if up {
        if block.start == 0 {
            return None;
        }
        let above = line_block(text, &(block.start - 1..block.start - 1));
        let above_text = &text[above.clone()];
        let shift = above_text.len() + 1;
        Some(LineEdit {
            range: above.start..block.end,
            text: format!("{moved}\n{above_text}"),
            selection: selection.start - shift..selection.end - shift,
        })
    } else {
        if block.end >= text.len() {
            return None;
        }
        let below = line_block(text, &(block.end + 1..block.end + 1));
        let below_text = &text[below.clone()];
        let shift = below_text.len() + 1;
        Some(LineEdit {
            range: block.start..below.end,
            text: format!("{below_text}\n{moved}"),
            selection: selection.start + shift..selection.end + shift,
        })
    }
}

/// Notepad++'s Ctrl+T: swap the caret's line with the one above, the caret staying on the same
/// line number (so it ends on the line that moved down). `None` on the first line.
pub(super) fn swap_with_previous(text: &str, selection: Range<usize>) -> Option<LineEdit> {
    let caret = selection.start..selection.start;
    let edit = move_lines(text, caret.clone(), true)?;
    // The caret's line moved up; keep the caret on the original line number instead, at the same
    // column of the line now there.
    let block = line_block(text, &caret);
    let column = caret.start - block.start;
    let above = line_block(text, &(block.start - 1..block.start - 1));
    let above_len = above.end - above.start;
    let moved_len = block.end - block.start;
    let line_start = above.start + moved_len + 1;
    let at = line_start + column.min(above_len);
    Some(LineEdit {
        selection: at..at,
        ..edit
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, edit: &LineEdit) -> String {
        let mut out = text.to_string();
        out.replace_range(edit.range.clone(), &edit.text);
        out
    }

    #[test]
    fn toggle_comment_adds_dashes_at_common_indent_and_removes_again() {
        let text = "SELECT 1;\n  SELECT 2;\n\nSELECT 3;";
        let edit = toggle_comment(text, 0..text.len(), false);
        let commented = apply(text, &edit);
        assert_eq!(commented, "-- SELECT 1;\n--   SELECT 2;\n\n-- SELECT 3;");
        let edit = toggle_comment(&commented, 0..commented.len(), false);
        assert_eq!(apply(&commented, &edit), text);
    }

    #[test]
    fn mysql_gets_navicats_hash_marker() {
        let text = "a\nb";
        let edit = toggle_comment(text, 0..text.len(), true);
        assert_eq!(apply(text, &edit), "# a\n# b");
    }

    #[test]
    fn toggle_comment_on_the_caret_line_only() {
        let text = "a\nb\nc";
        let edit = toggle_comment(text, 2..2, false);
        assert_eq!(apply(text, &edit), "a\n-- b\nc");
    }

    #[test]
    fn every_known_marker_is_uncommented() {
        assert_eq!(apply("-- x", &toggle_comment("-- x", 0..0, false)), "x");
        assert_eq!(apply("// x", &toggle_comment("// x", 0..0, false)), "x");
        assert_eq!(apply("# x", &toggle_comment("# x", 0..0, true)), "x");
        // `#` is not a comment outside MySQL / MariaDB.
        assert_eq!(apply("# x", &toggle_comment("# x", 0..0, false)), "-- # x");
    }

    #[test]
    fn a_selection_inside_a_line_gets_a_block_comment() {
        let text = "SELECT a, b FROM t";
        let edit = toggle_comment(text, 10..11, false);
        let commented = apply(text, &edit);
        assert_eq!(commented, "SELECT a, /* b */ FROM t");
        let edit = toggle_comment(&commented, 10..17, false);
        assert_eq!(apply(&commented, &edit), text);
    }

    #[test]
    fn a_selection_ending_at_a_line_start_excludes_that_line() {
        let text = "a\nb\nc";
        let edit = toggle_comment(text, 0..2, false);
        assert_eq!(apply(text, &edit), "-- a\nb\nc");
    }

    #[test]
    fn duplicate_line_keeps_the_column() {
        let text = "ab\ncd";
        let edit = duplicate(text, 1..1);
        assert_eq!(apply(text, &edit), "ab\nab\ncd");
        assert_eq!(edit.selection, 4..4);
    }

    #[test]
    fn duplicate_selection_selects_the_copy() {
        let text = "xyz";
        let edit = duplicate(text, 0..2);
        assert_eq!(apply(text, &edit), "xyxyz");
        assert_eq!(edit.selection, 2..4);
    }

    #[test]
    fn move_lines_up_and_down_keep_the_selection_on_them() {
        let text = "a\nbb\nc";
        let up = move_lines(text, 3..3, true).expect("up");
        assert_eq!(apply(text, &up), "bb\na\nc");
        assert_eq!(up.selection, 1..1);
        let down = move_lines(text, 3..3, false).expect("down");
        assert_eq!(apply(text, &down), "a\nc\nbb");
        assert_eq!(down.selection, 5..5);
        assert!(move_lines(text, 0..0, true).is_none());
        assert!(move_lines(text, 5..5, false).is_none());
    }

    #[test]
    fn swap_with_previous_keeps_the_caret_line_number() {
        let text = "first\nsecond";
        let edit = swap_with_previous(text, 8..8).expect("swap");
        assert_eq!(apply(text, &edit), "second\nfirst");
        // Line 2, column 2 of "first".
        assert_eq!(edit.selection, 9..9);
        assert!(swap_with_previous(text, 0..0).is_none());
    }
}
