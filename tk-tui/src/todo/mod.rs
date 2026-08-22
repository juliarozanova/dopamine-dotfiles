//! The aggregate checklist: one list over every open ticket's TODO section
//! plus the local file.

pub mod local;
pub mod model;

use crate::ui::todo as render;
use anyhow::Result;
use local::LocalFile;
use model::{Origin, TodoGroup};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

pub struct TodoView {
    pub groups: Vec<TodoGroup>,
    local: LocalFile,
    /// Index into the flattened item list, not into rows.
    pub cursor: usize,
    pub scroll: u16,
    pub status: Option<String>,
    pub pending_g: bool,
}

impl TodoView {
    pub fn new() -> Result<Self> {
        let local = LocalFile::load()?;
        let mut v = Self {
            groups: Vec::new(),
            local,
            cursor: 0,
            scroll: 0,
            status: None,
            pending_g: false,
        };
        v.rebuild();
        Ok(v)
    }

    /// Re-derive the groups from the backends. Cursor is clamped rather than
    /// reset, so a refresh doesn't lose your place.
    fn rebuild(&mut self) {
        self.groups = vec![self.local.group()];
        let n = self.item_count();
        if self.cursor >= n {
            self.cursor = n.saturating_sub(1);
        }
    }

    pub fn reload(&mut self) {
        match LocalFile::load() {
            Ok(f) => {
                self.local = f;
                self.rebuild();
                self.status = Some("refreshed".into());
            }
            Err(e) => self.status = Some(format!("reload failed: {e:#}")),
        }
    }

    pub fn item_count(&self) -> usize {
        self.groups.iter().map(|g| g.items.len()).sum()
    }

    /// (group, item) of the cursor.
    fn at(&self, nth: usize) -> Option<(usize, usize)> {
        let mut seen = 0;
        for (gi, g) in self.groups.iter().enumerate() {
            if nth < seen + g.items.len() {
                return Some((gi, nth - seen));
            }
            seen += g.items.len();
        }
        None
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let n = self.item_count();
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + delta).clamp(0, n as i32 - 1) as usize;
    }

    /// Jump to the first item of the next/previous group.
    pub fn move_group(&mut self, dir: i32) {
        let Some((gi, _)) = self.at(self.cursor) else {
            return;
        };
        let target = (gi as i32 + dir).clamp(0, self.groups.len() as i32 - 1) as usize;
        let mut first = 0;
        for g in &self.groups[..target] {
            first += g.items.len();
        }
        if self.groups[target].items.is_empty() {
            return;
        }
        self.cursor = first;
    }

    /// Flip the cursor item and persist it. Local writes are synchronous —
    /// it's one file — so there's nothing optimistic to undo here.
    pub fn toggle(&mut self) {
        let Some((gi, ii)) = self.at(self.cursor) else {
            return;
        };
        let item = &self.groups[gi].items[ii];
        let want = !item.done;
        match item.origin.clone() {
            Origin::Local { line } => {
                self.local.set_done(line, want);
                match self.local.save() {
                    Ok(()) => self.groups[gi].items[ii].done = want,
                    Err(e) => {
                        // Put the file back the way it was in memory, so a
                        // later successful save doesn't carry a stale edit.
                        self.local.set_done(line, !want);
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Origin::Jira { .. } => {
                self.status = Some("jira sync not wired up yet".into());
            }
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(f.area());
        let view_h = chunks[0].height;

        let rows = render::rows(&self.groups);
        let mut lines: Vec<Line> = rows.iter().map(|r| r.line.clone()).collect();

        // keep the cursor row on screen, then paint it
        if let Some(row) = render::row_of(&rows, self.cursor) {
            let row = row as u16;
            if row < self.scroll {
                self.scroll = row;
            } else if view_h > 0 && row >= self.scroll + view_h {
                self.scroll = row + 1 - view_h;
            }
            let th = crate::theme::theme();
            lines[row as usize].style = lines[row as usize]
                .style
                .bg(th.selection)
                .fg(th.inverted);
        }
        let max = (lines.len() as u16).saturating_sub(view_h.max(1));
        self.scroll = self.scroll.min(max);

        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            chunks[0],
        );

        let th = crate::theme::theme();
        let footer = match &self.status {
            Some(s) => Line::styled(format!(" {s}"), Style::default().fg(th.status)),
            None => Line::from(vec![Span::styled(
                " j/k move · J/K group · space done · r refresh · q quit".to_string(),
                Style::default().fg(th.dim),
            )]),
        };
        f.render_widget(Paragraph::new(footer), chunks[1]);
    }

    pub fn dump(&self) -> Vec<String> {
        render::plain_dump(&self.groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::TodoItem;

    fn view(groups: Vec<TodoGroup>) -> TodoView {
        TodoView {
            groups,
            local: LocalFile::load_from("/nonexistent/todo.md".into()).unwrap(),
            cursor: 0,
            scroll: 0,
            status: None,
            pending_g: false,
        }
    }

    fn item(text: &str) -> TodoItem {
        TodoItem {
            text: text.into(),
            done: false,
            origin: Origin::Local { line: 0 },
            dirty: false,
        }
    }

    fn two_groups() -> Vec<TodoGroup> {
        vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("a"), item("b")],
            },
            TodoGroup {
                title: "t".into(),
                key: Some("JROZ-1".into()),
                items: vec![item("c")],
            },
        ]
    }

    #[test]
    fn the_cursor_walks_items_across_group_boundaries() {
        let mut v = view(two_groups());
        assert_eq!(v.at(0), Some((0, 0)));
        assert_eq!(v.at(2), Some((1, 0)));
        v.move_cursor(5);
        assert_eq!(v.cursor, 2, "clamps at the last item");
        v.move_cursor(-99);
        assert_eq!(v.cursor, 0);
    }

    #[test]
    fn group_jumps_land_on_the_first_item_of_the_group() {
        let mut v = view(two_groups());
        v.move_group(1);
        assert_eq!(v.cursor, 2);
        v.move_group(-1);
        assert_eq!(v.cursor, 0);
    }

    #[test]
    fn an_empty_list_does_not_panic() {
        let mut v = view(Vec::new());
        v.move_cursor(1);
        v.move_group(1);
        v.toggle();
        assert_eq!(v.item_count(), 0);
    }
}
