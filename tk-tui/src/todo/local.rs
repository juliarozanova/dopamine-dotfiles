//! `~/Dashboard/todo.md` — the ticket-less half of the list.
//!
//! Markdown, deliberately: you own the whole file, so the surgical-edit
//! property that justifies keeping Jira's ADF intact buys nothing here. What
//! matters instead is that the file stays greppable, diffable, and editable in
//! nvim in the next pane. Even so we rewrite only the checkbox lines — prose
//! and headings around them survive verbatim.

use super::model::{Origin, TodoGroup, TodoItem};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct LocalFile {
    path: PathBuf,
    lines: Vec<String>,
    /// mtime as read, so a save can refuse to clobber an outside edit.
    mtime: Option<SystemTime>,
    /// Whether the file ended with a newline, so we round-trip it.
    trailing_newline: bool,
}

/// `$DASHBOARD/todo.md`, else `~/Dashboard/todo.md`.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("DASHBOARD")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Dashboard")))
        .unwrap_or_default();
    base.join("todo.md")
}

/// Split a markdown checkbox line into (indent+bullet, done, text).
/// Accepts `-`, `*` and `+` bullets and either case of the tick.
fn parse_checkbox(line: &str) -> Option<(usize, bool, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let bullet = rest.chars().next()?;
    if !matches!(bullet, '-' | '*' | '+') {
        return None;
    }
    let rest = rest[1..].strip_prefix(' ')?;
    let rest = rest.strip_prefix('[')?;
    let mark = rest.chars().next()?;
    let rest = rest[mark.len_utf8()..].strip_prefix(']')?;
    let done = match mark {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    // A bare "- [ ]" with no trailing space is still an (empty) item.
    let text = rest.strip_prefix(' ').unwrap_or(rest);
    Some((indent, done, text))
}

/// Two spaces per level, the markdown convention. Indentation that doesn't
/// divide evenly still round-trips, because edits reuse the line's own indent
/// — only new items are written at a computed depth.
pub const INDENT: usize = 2;

fn render_checkbox(indent: usize, done: bool, text: &str) -> String {
    format!(
        "{}- [{}] {}",
        " ".repeat(indent),
        if done { 'x' } else { ' ' },
        text
    )
}

fn mtime_of(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

impl LocalFile {
    pub fn load() -> Result<Self> {
        Self::load_from(default_path())
    }

    pub fn load_from(path: PathBuf) -> Result<Self> {
        // A missing file is not an error — it's an empty list you can add to.
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).context(format!("reading {}", path.display())),
        };
        let trailing_newline = raw.is_empty() || raw.ends_with('\n');
        let lines = raw
            .strip_suffix('\n')
            .unwrap_or(&raw)
            .split('\n')
            .map(str::to_string)
            .collect::<Vec<_>>();
        let lines = if raw.is_empty() { Vec::new() } else { lines };
        let mtime = mtime_of(&path);
        Ok(Self {
            path,
            lines,
            mtime,
            trailing_newline,
        })
    }

    pub fn items(&self) -> Vec<TodoItem> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                parse_checkbox(l).map(|(indent, done, text)| TodoItem {
                    text: text.to_string(),
                    done,
                    origin: Origin::Local { line: i },
                    dirty: false,
                    depth: indent / INDENT,
                })
            })
            .collect()
    }

    pub fn group(&self) -> TodoGroup {
        TodoGroup {
            title: "no ticket".to_string(),
            key: None,
            items: self.items(),
        }
    }

    /// Rewrite one checkbox line, preserving its indent. Silently ignores a
    /// line that isn't a checkbox — the caller's index came from `items()`, so
    /// that only happens if the file changed underneath us, and `save` is where
    /// that gets caught.
    fn edit(&mut self, line: usize, done: Option<bool>, text: Option<&str>) {
        let Some(existing) = self.lines.get(line) else {
            return;
        };
        let Some((indent, was_done, was_text)) = parse_checkbox(existing) else {
            return;
        };
        let done = done.unwrap_or(was_done);
        let text = text.unwrap_or(was_text).to_string();
        self.lines[line] = render_checkbox(indent, done, &text);
    }

    pub fn set_done(&mut self, line: usize, done: bool) {
        self.edit(line, Some(done), None);
    }

    pub fn set_text(&mut self, line: usize, text: &str) {
        self.edit(line, None, Some(text));
    }

    pub fn remove(&mut self, line: usize) {
        if line < self.lines.len() {
            self.lines.remove(line);
        }
    }

    /// Insert a new unticked item after the last existing checkbox (so items
    /// stay together under whatever heading they're already under), or at the
    /// end of the file if there are none yet. Returns its line index.
    /// Insert a new unticked item. `after_line` puts it directly below that
    /// line (so `o` adds where you're standing); otherwise it goes after the
    /// last checkbox, keeping items together. `depth` overrides the indent it
    /// would otherwise inherit.
    pub fn insert_at(
        &mut self,
        text: &str,
        after_line: Option<usize>,
        depth: Option<usize>,
    ) -> usize {
        let after = match after_line {
            Some(l) if l < self.lines.len() => l + 1,
            _ => self
                .lines
                .iter()
                .rposition(|l| parse_checkbox(l).is_some())
                .map(|i| i + 1)
                .unwrap_or(self.lines.len()),
        };
        let inherited = self
            .lines
            .get(after.saturating_sub(1))
            .and_then(|l| parse_checkbox(l))
            .map(|(i, _, _)| i)
            .unwrap_or(0);
        let indent = depth.map(|d| d * INDENT).unwrap_or(inherited);
        self.lines.insert(after, render_checkbox(indent, false, text));
        after
    }

    /// Re-indent a checkbox line by `delta` levels, clamped at the left margin.
    /// Returns the new depth.
    pub fn shift(&mut self, line: usize, delta: i32) -> Option<usize> {
        let (indent, done, text) = self.lines.get(line).and_then(|l| parse_checkbox(l))?;
        let depth = (indent / INDENT) as i32;
        let next = (depth + delta).max(0) as usize;
        let text = text.to_string();
        self.lines[line] = render_checkbox(next * INDENT, done, &text);
        Some(next)
    }

    /// True if the file on disk has moved on since we read it.
    pub fn stale(&self) -> bool {
        match (self.mtime, mtime_of(&self.path)) {
            (Some(then), Some(now)) => then != now,
            // File appeared or vanished under us — treat as stale either way.
            (a, b) => a.is_some() != b.is_some(),
        }
    }

    /// Write back. Refuses if the file changed underneath us, since you may
    /// well have it open in nvim in the next pane.
    pub fn save(&mut self) -> Result<()> {
        if self.stale() {
            anyhow::bail!("todo.md changed on disk — press r to reload");
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let mut out = self.lines.join("\n");
        if self.trailing_newline && !out.is_empty() {
            out.push('\n');
        }
        std::fs::write(&self.path, out)
            .context(format!("writing {}", self.path.display()))?;
        self.mtime = mtime_of(&self.path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(body: &str) -> LocalFile {
        LocalFile {
            path: PathBuf::from("/nonexistent/todo.md"),
            lines: body.lines().map(str::to_string).collect(),
            mtime: None,
            trailing_newline: true,
        }
    }

    #[test]
    fn parses_the_bullet_styles_that_turn_up_in_markdown() {
        assert_eq!(parse_checkbox("- [ ] plain"), Some((0, false, "plain")));
        assert_eq!(parse_checkbox("- [x] ticked"), Some((0, true, "ticked")));
        assert_eq!(parse_checkbox("* [X] star"), Some((0, true, "star")));
        assert_eq!(parse_checkbox("  + [ ] nested"), Some((2, false, "nested")));
        assert_eq!(parse_checkbox("- [ ]"), Some((0, false, "")));
    }

    #[test]
    fn ignores_lines_that_are_not_checkboxes() {
        for line in ["# heading", "- a plain bullet", "prose", "", "-[ ] no space"] {
            assert_eq!(parse_checkbox(line), None, "should ignore {line:?}");
        }
    }

    /// The whole point of editing in place: everything that isn't a checkbox
    /// comes back byte-identical.
    #[test]
    fn rewriting_an_item_leaves_surrounding_prose_verbatim() {
        let mut f = file("# Todo\n\nsome prose here\n\n- [ ] first\n- [ ] second\n\n## Notes\ntrailing prose");
        f.set_done(4, true);
        f.set_text(5, "second, reworded");

        assert_eq!(
            f.lines.join("\n"),
            "# Todo\n\nsome prose here\n\n- [x] first\n- [ ] second, reworded\n\n## Notes\ntrailing prose"
        );
    }

    #[test]
    fn editing_preserves_indent() {
        let mut f = file("  - [ ] nested");
        f.set_done(0, true);
        assert_eq!(f.lines[0], "  - [x] nested");
    }

    #[test]
    fn items_carry_their_line_index_as_identity() {
        let f = file("# Todo\n- [ ] first\nprose\n- [x] second");
        let items = f.items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].origin, Origin::Local { line: 1 });
        assert_eq!(items[1].origin, Origin::Local { line: 3 });
        assert!(items[1].done);
    }

    #[test]
    fn insert_lands_after_the_last_checkbox_not_at_eof() {
        let mut f = file("- [ ] first\n- [ ] second\n\n## Notes\nprose");
        let line = f.insert_at("third", None, None);
        assert_eq!(line, 2);
        assert_eq!(f.lines[2], "- [ ] third");
        assert_eq!(f.lines.last().unwrap(), "prose");
    }

    #[test]
    fn insert_into_a_file_with_no_checkboxes_appends() {
        let mut f = file("# Todo\n\nprose");
        assert_eq!(f.insert_at("first", None, None), 3);
        assert_eq!(f.lines[3], "- [ ] first");
    }

    /// The in-memory tests above cover the edit; this covers the bytes that
    /// actually hit the disk, including the trailing newline.
    #[test]
    fn depth_comes_from_the_indent_and_survives_a_round_trip() {
        let f = file("- [ ] top\n  - [ ] child\n    - [x] grandchild\n- [ ] back to top");
        let depths: Vec<_> = f.items().iter().map(|i| i.depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 0]);
    }

    #[test]
    fn shifting_reindents_and_clamps_at_the_margin() {
        let mut f = file("- [ ] top\n  - [ ] child");
        assert_eq!(f.shift(1, -1), Some(0));
        assert_eq!(f.lines[1], "- [ ] child");
        assert_eq!(f.shift(1, -1), Some(0), "cannot go left of the margin");
        assert_eq!(f.shift(1, 1), Some(1));
        assert_eq!(f.lines[1], "  - [ ] child");
        assert_eq!(f.shift(0, 1), Some(1), "indent is the caller's business");
    }

    #[test]
    fn a_new_item_can_land_below_the_one_you_are_on_at_a_chosen_depth() {
        let mut f = file("- [ ] first\n- [ ] second\n\n## Notes\nprose");
        let line = f.insert_at("nested under first", Some(0), Some(1));
        assert_eq!(line, 1);
        assert_eq!(f.lines[1], "  - [ ] nested under first");
        assert_eq!(f.lines[2], "- [ ] second", "nothing else moved");
        assert_eq!(f.lines.last().unwrap(), "prose");
    }

    #[test]
    fn a_real_round_trip_through_the_filesystem_only_changes_the_checkbox() {
        let path = std::env::temp_dir().join(format!("tk-tui-test-{}.md", std::process::id()));
        let before = "# Todo\n\nprose\n\n- [ ] first\n- [ ] second\n\n## Notes\ntail\n";
        std::fs::write(&path, before).unwrap();

        let mut f = LocalFile::load_from(path.clone()).unwrap();
        f.set_done(4, true);
        f.save().unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, before.replace("- [ ] first", "- [x] first"));

        // and saving again is a no-op rather than a stale-file bail
        f.save().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_save_refuses_to_clobber_an_outside_edit() {
        let path = std::env::temp_dir().join(format!("tk-tui-stale-{}.md", std::process::id()));
        std::fs::write(&path, "- [ ] first\n").unwrap();
        let mut f = LocalFile::load_from(path.clone()).unwrap();

        // someone edits it in nvim in the next pane
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, "- [ ] first\n- [ ] added elsewhere\n").unwrap();

        f.set_done(0, true);
        let err = f.save().unwrap_err().to_string();
        assert!(err.contains("changed on disk"), "got: {err}");
        assert!(std::fs::read_to_string(&path).unwrap().contains("added elsewhere"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_list() {
        let f = LocalFile::load_from(PathBuf::from("/nonexistent/nope/todo.md")).unwrap();
        assert!(f.items().is_empty());
        assert!(f.group().is_local());
    }
}
