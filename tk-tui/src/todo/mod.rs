//! The aggregate checklist: one list over every open ticket's TODO section
//! plus the local file.

pub mod jira;
pub mod local;
pub mod sync;
pub mod model;

use crate::editor::{EditMode, Editor, Outcome};
use crate::rest::Config;
use crate::ui::todo as render;
use anyhow::Result;
use local::LocalFile;
use model::{Origin, TodoGroup};
use sync::{Op, Sync};
use serde_json::Value;
use std::collections::HashMap;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::crossterm::event::KeyEvent;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

/// What an in-progress edit will become when it commits.
enum Target {
    /// Retext the item at this flat cursor index.
    Existing(usize),
    /// Add a new item to this group.
    New(usize),
}

struct Edit {
    editor: Editor,
    target: Target,
}

pub struct TodoView {
    pub groups: Vec<TodoGroup>,
    local: LocalFile,
    /// None when Jira isn't configured or reachable — the local list still
    /// works, which is the whole reason the dash pane can't hard-fail.
    cfg: Option<Config>,
    /// Each ticket's description exactly as fetched. Write-back mutates a
    /// clone of this rather than rebuilding a description from the model.
    docs: HashMap<String, Value>,
    /// Why the Jira half is missing, shown as a banner rather than an abort.
    jira_error: Option<String>,
    /// Index into the flattened item list, not into rows.
    pub cursor: usize,
    pub scroll: u16,
    pub status: Option<String>,
    pub pending_g: bool,
    /// A pending `d` waiting for its second `d`.
    pub pending_d: bool,
    editing: Option<Edit>,
    /// Spawned on the first Jira write, so a local-only session never starts
    /// a thread it doesn't need.
    syncer: Option<Sync>,
    /// Writes queued but not yet acknowledged.
    in_flight: usize,
}

impl TodoView {
    pub fn new() -> Result<Self> {
        let local = LocalFile::load()?;
        let mut v = Self {
            groups: Vec::new(),
            local,
            cfg: None,
            docs: HashMap::new(),
            jira_error: None,
            cursor: 0,
            scroll: 0,
            status: None,
            pending_g: false,
            pending_d: false,
            editing: None,
            syncer: None,
            in_flight: 0,
        };
        v.rebuild();
        Ok(v)
    }

    /// Re-derive the groups from both backends. Cursor is clamped rather than
    /// reset, so a refresh doesn't lose your place.
    ///
    /// A Jira failure is recorded, not propagated: an expired token should cost
    /// you the ticket groups and a banner, not the whole pane.
    fn rebuild(&mut self) {
        let mut groups = vec![self.local.group()];
        self.docs.clear();
        self.jira_error = None;

        match self.fetch_jira() {
            Ok(issues) => {
                for issue in &issues {
                    if let Some(d) = &issue.description {
                        self.docs.insert(issue.key.clone(), d.clone());
                    }
                    groups.push(jira::group(issue));
                }
            }
            Err(e) => self.jira_error = Some(format!("{e:#}")),
        }

        self.groups = groups;
        let n = self.item_count();
        if self.cursor >= n {
            self.cursor = n.saturating_sub(1);
        }
    }

    fn fetch_jira(&mut self) -> Result<Vec<jira::Issue>> {
        if self.cfg.is_none() {
            self.cfg = Some(crate::rest::config()?);
        }
        let cfg = self.cfg.as_ref().expect("just set");
        jira::search(cfg)
    }

    /// The description a ticket had when we last read it.
    pub fn doc_for(&self, key: &str) -> Option<&Value> {
        self.docs.get(key)
    }

    pub fn reload(&mut self) {
        match LocalFile::load() {
            Ok(f) => {
                self.local = f;
                self.rebuild();
                self.status = Some(match &self.jira_error {
                    Some(e) => format!("local reloaded — jira: {e}"),
                    None => "refreshed".into(),
                });
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
            Origin::Jira { key, local_id } => {
                self.groups[gi].items[ii].done = want;
                self.push_jira(
                    gi,
                    ii,
                    Op::Done {
                        key,
                        local_id,
                        done: want,
                        was: !want,
                    },
                );
            }
        }
    }

    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Start editing the item under the cursor.
    pub fn edit_item(&mut self, mode: EditMode) {
        let Some((gi, ii)) = self.at(self.cursor) else {
            self.status = Some("nothing to edit — o to add".into());
            return;
        };
        let text = self.groups[gi].items[ii].text.clone();
        self.editing = Some(Edit {
            editor: Editor::new(&text, mode),
            target: Target::Existing(self.cursor),
        });
    }

    /// Start a new item in the group the cursor is in.
    pub fn new_item(&mut self) {
        let gi = self.at(self.cursor).map(|(g, _)| g).unwrap_or(0);
        if self.groups.is_empty() {
            return;
        }
        let mut editor = Editor::new("", EditMode::Insert);
        editor.fresh = true;
        self.editing = Some(Edit {
            editor,
            target: Target::New(gi),
        });
    }

    /// Feed a key to the open editor. Returns true if it was consumed.
    pub fn edit_key(&mut self, k: KeyEvent) -> bool {
        let Some(edit) = self.editing.as_mut() else {
            return false;
        };
        match edit.editor.key(k) {
            Outcome::Continue => {}
            Outcome::Cancel => {
                self.editing = None;
                self.status = Some("edit discarded".into());
            }
            Outcome::Commit => {
                let edit = self.editing.take().expect("checked above");
                let text = edit.editor.text().trim().to_string();
                match edit.target {
                    _ if text.is_empty() => {
                        self.status = Some("empty — nothing saved".into());
                    }
                    Target::Existing(nth) => self.commit_text(nth, &text),
                    Target::New(gi) => self.commit_new(gi, &text),
                }
            }
        }
        true
    }

    /// Save retexted item `nth`. An unchanged text is a deliberate no-op: on
    /// the Jira side rewriting an item flattens any marks inside it, so an item
    /// you opened and closed without typing must never be written back.
    fn commit_text(&mut self, nth: usize, text: &str) {
        let Some((gi, ii)) = self.at(nth) else { return };
        if self.groups[gi].items[ii].text == text {
            self.status = Some("unchanged".into());
            return;
        }
        match self.groups[gi].items[ii].origin.clone() {
            Origin::Local { line } => {
                self.local.set_text(line, text);
                match self.local.save() {
                    Ok(()) => self.groups[gi].items[ii].text = text.to_string(),
                    Err(e) => {
                        self.local.set_text(line, &self.groups[gi].items[ii].text.clone());
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Origin::Jira { key, local_id } => {
                let was = self.groups[gi].items[ii].text.clone();
                self.groups[gi].items[ii].text = text.to_string();
                self.push_jira(
                    gi,
                    ii,
                    Op::Text {
                        key,
                        local_id,
                        text: text.to_string(),
                        was,
                    },
                );
            }
        }
    }

    fn commit_new(&mut self, gi: usize, text: &str) {
        match self.groups[gi].key.clone() {
            None => {
                let line = self.local.insert(text);
                match self.local.save() {
                    Ok(()) => {
                        self.rebuild();
                        // land the cursor on what you just wrote
                        if let Some(n) = self.flat_index_of(&Origin::Local { line }) {
                            self.cursor = n;
                        }
                    }
                    Err(e) => {
                        self.local.remove(line);
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Some(key) => {
                // No optimistic row for an add: the item has no localId until
                // Jira mints one, so we show it on the refresh that follows.
                self.queue(Op::Add {
                    key,
                    text: text.to_string(),
                });
                self.status = Some(format!("adding “{text}”…"));
            }
        }
    }

    fn flat_index_of(&self, origin: &Origin) -> Option<usize> {
        let mut n = 0;
        for g in &self.groups {
            for it in &g.items {
                if &it.origin == origin {
                    return Some(n);
                }
                n += 1;
            }
        }
        None
    }

    fn queue(&mut self, op: Op) {
        if self.syncer.is_none() {
            self.syncer = Some(Sync::spawn());
        }
        self.in_flight += 1;
        self.syncer.as_ref().expect("just set").send(op);
    }

    /// Optimistically mark an item in flight and queue its write.
    fn push_jira(&mut self, gi: usize, ii: usize, op: Op) {
        self.groups[gi].items[ii].dirty = true;
        self.queue(op);
    }

    /// Drain finished writes. A failure reloads rather than guessing at a
    /// rollback: whatever Jira says is the truth, and the message says why.
    pub fn poll_sync(&mut self) {
        let Some(syncer) = &self.syncer else { return };
        let done = syncer.poll();
        if done.is_empty() {
            return;
        }
        let mut failure = None;
        let mut ok = 0;
        for d in done {
            self.in_flight = self.in_flight.saturating_sub(1);
            match d.error {
                None => {
                    ok += 1;
                    if let Some(id) = d.op.local_id() {
                        self.clear_dirty(d.op.key(), id);
                    }
                }
                Some(e) => failure = Some(e),
            }
        }
        match failure {
            Some(e) => {
                self.reload();
                self.status = Some(e);
            }
            None if ok > 0 => {
                // an Add has no local id to reconcile, so pull it back down
                self.reload();
                self.status = Some("synced ✓".into());
            }
            None => {}
        }
    }

    fn clear_dirty(&mut self, key: &str, local_id: &str) {
        for g in &mut self.groups {
            for it in &mut g.items {
                if it.origin == (Origin::Jira { key: key.to_string(), local_id: local_id.to_string() })
                {
                    it.dirty = false;
                }
            }
        }
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Delete the item under the cursor.
    pub fn delete_item(&mut self) {
        let Some((gi, ii)) = self.at(self.cursor) else {
            return;
        };
        match self.groups[gi].items[ii].origin.clone() {
            Origin::Local { line } => {
                let snapshot = self.groups[gi].items[ii].clone();
                self.local.remove(line);
                match self.local.save() {
                    Ok(()) => {
                        self.rebuild();
                        self.status = Some(format!("deleted “{}”", snapshot.text));
                    }
                    Err(e) => {
                        self.reload();
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Origin::Jira { key, local_id } => {
                self.push_jira(gi, ii, Op::Delete { key, local_id })
            }

        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(f.area());
        let view_h = chunks[0].height;

        let rows = render::rows(&self.groups);
        let mut lines: Vec<Line> = rows.iter().map(|r| r.line.clone()).collect();

        // the row being edited is replaced by the live buffer
        let editing_row = self.editing.as_ref().and_then(|e| {
            let nth = match e.target {
                Target::Existing(nth) => nth,
                // a new item shows at the end of its group
                Target::New(gi) => {
                    let mut n = 0;
                    for g in &self.groups[..gi] {
                        n += g.items.len();
                    }
                    n + self.groups.get(gi).map(|g| g.items.len()).unwrap_or(0)
                }
            };
            let row = render::row_of(&rows, nth).or_else(|| {
                // an empty group has no item rows; fall back to just after its
                // heading, which is the placeholder line
                render::row_of(&rows, nth.saturating_sub(1)).map(|r| r + 1)
            })?;
            Some((row, e))
        });
        if let Some((row, e)) = editing_row {
            let line = render::edit_line(
                &e.editor.text(),
                e.editor.cursor(),
                e.editor.mode() == EditMode::Insert,
            );
            if let Some(slot) = lines.get_mut(row) {
                *slot = line;
            } else {
                lines.push(line);
            }
        }

        // keep the cursor row on screen, then paint it — except while editing,
        // where the caret is the highlight
        let paint = editing_row.map(|(r, _)| r).or_else(|| render::row_of(&rows, self.cursor));
        let highlight = editing_row.is_none();
        if let Some(row) = paint {
            let row = row as u16;
            if row < self.scroll {
                self.scroll = row;
            } else if view_h > 0 && row >= self.scroll + view_h {
                self.scroll = row + 1 - view_h;
            }
            if highlight {
                let th = crate::theme::theme();
                lines[row as usize].style = lines[row as usize]
                    .style
                    .bg(th.selection)
                    .fg(th.inverted);
            }
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
        let footer = match (&self.status, &self.jira_error) {
            _ if self.editing.is_some() => Line::styled(
                " esc discard · ⏎/ZZ save · i insert · vim motions".to_string(),
                Style::default().fg(th.dim),
            ),
            (Some(s), _) => Line::styled(format!(" {s}"), Style::default().fg(th.status)),
            (None, Some(e)) => Line::styled(
                format!(" jira unavailable: {e}"),
                Style::default().fg(th.pending),
            ),
            (None, None) => Line::from(vec![Span::styled(
                " j/k move · space done · i edit · o new · dd delete · r refresh · q quit"
                    .to_string(),
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
            cfg: None,
            docs: HashMap::new(),
            jira_error: None,
            cursor: 0,
            scroll: 0,
            status: None,
            pending_g: false,
            pending_d: false,
            editing: None,
            syncer: None,
            in_flight: 0,
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
