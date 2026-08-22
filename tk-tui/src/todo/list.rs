//! The checklist widget: cursor, modal editing, and the write dispatch.
//!
//! Shared by the aggregate view (many groups, plus the local file) and the
//! ticket pane's TODO mode (one group, Jira only), so the two can't drift.

use super::local::LocalFile;
use super::model::{Origin, TodoGroup};
use super::sync::{Op, Sync};
use crate::editor::{EditMode, Editor, Outcome};
use crate::ui::todo as render;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

/// What an in-progress edit becomes when it commits.
enum Target {
    Existing(usize),
    New(usize),
}

struct Edit {
    editor: Editor,
    target: Target,
}

/// What the host should do after a key the list didn't fully handle.
#[derive(Debug, PartialEq, Eq)]
pub enum ListAction {
    None,
    /// Leave this view.
    Quit,
    /// Open the ticket the cursor is on.
    Open(String),
    /// Reload from the backends.
    Reload,
}

pub struct ItemList {
    pub groups: Vec<TodoGroup>,
    pub cursor: usize,
    pub scroll: u16,
    pub status: Option<String>,
    /// The local markdown file, in the aggregate view only. The ticket pane's
    /// TODO mode has no ticket-less group.
    pub local: Option<LocalFile>,
    pending_g: bool,
    pending_d: bool,
    editing: Option<Edit>,
    /// Choosing a ticket to promote a local item into.
    picking: Option<(usize, usize)>, // (flat item index, highlighted group)
    syncer: Option<Sync>,
    in_flight: usize,
}

impl ItemList {
    pub fn new(local: Option<LocalFile>) -> Self {
        Self {
            groups: Vec::new(),
            cursor: 0,
            scroll: 0,
            status: None,
            local,
            pending_g: false,
            pending_d: false,
            editing: None,
            picking: None,
            syncer: None,
            in_flight: 0,
        }
    }

    pub fn item_count(&self) -> usize {
        self.groups.iter().map(|g| g.items.len()).sum()
    }

    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Clamp the cursor after the groups change, keeping your place.
    pub fn clamp(&mut self) {
        let n = self.item_count();
        if self.cursor >= n {
            self.cursor = n.saturating_sub(1);
        }
    }

    pub fn at(&self, nth: usize) -> Option<(usize, usize)> {
        let mut seen = 0;
        for (gi, g) in self.groups.iter().enumerate() {
            if nth < seen + g.items.len() {
                return Some((gi, nth - seen));
            }
            seen += g.items.len();
        }
        None
    }

    fn first_of_group(&self, gi: usize) -> usize {
        self.groups[..gi].iter().map(|g| g.items.len()).sum()
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

    pub fn move_cursor(&mut self, delta: i32) {
        let n = self.item_count();
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + delta).clamp(0, n as i32 - 1) as usize;
    }

    pub fn move_group(&mut self, dir: i32) {
        let Some((gi, _)) = self.at(self.cursor) else {
            return;
        };
        let target = (gi as i32 + dir).clamp(0, self.groups.len() as i32 - 1) as usize;
        if self.groups[target].items.is_empty() {
            return;
        }
        self.cursor = self.first_of_group(target);
    }

    // ------------------------------------------------------------ writes --

    fn queue(&mut self, op: Op) {
        if self.syncer.is_none() {
            self.syncer = Some(Sync::spawn());
        }
        self.in_flight += 1;
        self.syncer.as_ref().expect("just set").send(op);
    }

    fn push_jira(&mut self, gi: usize, ii: usize, op: Op) {
        self.groups[gi].items[ii].dirty = true;
        self.queue(op);
    }

    pub fn toggle(&mut self) {
        let Some((gi, ii)) = self.at(self.cursor) else {
            return;
        };
        let want = !self.groups[gi].items[ii].done;
        match self.groups[gi].items[ii].origin.clone() {
            Origin::Local { line } => {
                let Some(local) = self.local.as_mut() else { return };
                local.set_done(line, want);
                match local.save() {
                    Ok(()) => self.groups[gi].items[ii].done = want,
                    Err(e) => {
                        self.local.as_mut().expect("checked").set_done(line, !want);
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Origin::Jira { key, local_id } => {
                self.groups[gi].items[ii].done = want;
                self.push_jira(gi, ii, Op::Done { key, local_id, done: want, was: !want });
            }
        }
    }

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

    pub fn new_item(&mut self) {
        if self.groups.is_empty() {
            return;
        }
        let gi = self.at(self.cursor).map(|(g, _)| g).unwrap_or(0);
        self.editing = Some(Edit {
            editor: Editor::new("", EditMode::Insert),
            target: Target::New(gi),
        });
    }

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
                if text.is_empty() {
                    self.status = Some("empty — nothing saved".into());
                } else {
                    match edit.target {
                        Target::Existing(nth) => self.commit_text(nth, &text),
                        Target::New(gi) => self.commit_new(gi, &text),
                    }
                }
            }
        }
        true
    }

    /// An unchanged text is a deliberate no-op: rewriting a Jira item flattens
    /// any marks inside it, so an item you opened and closed without typing
    /// must never be written back.
    fn commit_text(&mut self, nth: usize, text: &str) {
        let Some((gi, ii)) = self.at(nth) else { return };
        let was = self.groups[gi].items[ii].text.clone();
        if was == text {
            self.status = Some("unchanged".into());
            return;
        }
        match self.groups[gi].items[ii].origin.clone() {
            Origin::Local { line } => {
                let Some(local) = self.local.as_mut() else { return };
                local.set_text(line, text);
                match local.save() {
                    Ok(()) => self.groups[gi].items[ii].text = text.to_string(),
                    Err(e) => {
                        self.local.as_mut().expect("checked").set_text(line, &was);
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Origin::Jira { key, local_id } => {
                self.groups[gi].items[ii].text = text.to_string();
                self.push_jira(
                    gi,
                    ii,
                    Op::Text { key, local_id, text: text.to_string(), was },
                );
            }
        }
    }

    fn commit_new(&mut self, gi: usize, text: &str) {
        match self.groups[gi].key.clone() {
            None => {
                let Some(local) = self.local.as_mut() else { return };
                let line = local.insert(text);
                match local.save() {
                    Ok(()) => {
                        self.refresh_local();
                        if let Some(n) = self.flat_index_of(&Origin::Local { line }) {
                            self.cursor = n;
                        }
                    }
                    Err(e) => {
                        self.local.as_mut().expect("checked").remove(line);
                        self.status = Some(format!("{e:#}"));
                    }
                }
            }
            Some(key) => {
                // No optimistic row: the item has no localId until Jira mints
                // one, so it appears on the reload that follows.
                self.queue(Op::Add { key, text: text.to_string() });
                self.status = Some(format!("adding “{text}”…"));
            }
        }
    }

    pub fn delete_item(&mut self) {
        let Some((gi, ii)) = self.at(self.cursor) else {
            return;
        };
        match self.groups[gi].items[ii].origin.clone() {
            Origin::Local { line } => {
                let text = self.groups[gi].items[ii].text.clone();
                let Some(local) = self.local.as_mut() else { return };
                local.remove(line);
                match local.save() {
                    Ok(()) => {
                        self.refresh_local();
                        self.status = Some(format!("deleted “{text}”"));
                    }
                    Err(e) => self.status = Some(format!("{e:#}")),
                }
            }
            Origin::Jira { key, local_id } => {
                self.push_jira(gi, ii, Op::Delete { key, local_id })
            }
        }
    }

    /// Re-read the local file into its group, leaving the Jira groups alone.
    fn refresh_local(&mut self) {
        let Some(local) = self.local.as_ref() else { return };
        let group = local.group();
        if let Some(g) = self.groups.iter_mut().find(|g| g.is_local()) {
            *g = group;
        }
        self.clamp();
    }

    // -------------------------------------------------------- promotion ---

    /// Start choosing a ticket to move the cursor's local item into.
    pub fn promote(&mut self) {
        let Some((gi, ii)) = self.at(self.cursor) else { return };
        if !matches!(self.groups[gi].items[ii].origin, Origin::Local { .. }) {
            self.status = Some("already on a ticket".into());
            return;
        }
        let first_ticket = self.groups.iter().position(|g| !g.is_local());
        match first_ticket {
            Some(t) => self.picking = Some((self.cursor, t)),
            None => self.status = Some("no tickets to promote into".into()),
        }
    }

    pub fn picking(&self) -> bool {
        self.picking.is_some()
    }

    fn pick_key(&mut self, k: KeyEvent) -> bool {
        let Some((nth, mut hi)) = self.picking else {
            return false;
        };
        let tickets: Vec<usize> = (0..self.groups.len())
            .filter(|&i| !self.groups[i].is_local())
            .collect();
        let pos = tickets.iter().position(|&i| i == hi).unwrap_or(0);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.picking = None;
                self.status = Some("promotion cancelled".into());
            }
            KeyCode::Char('j') | KeyCode::Down => {
                hi = tickets[(pos + 1).min(tickets.len() - 1)];
                self.picking = Some((nth, hi));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                hi = tickets[pos.saturating_sub(1)];
                self.picking = Some((nth, hi));
            }
            KeyCode::Enter => {
                self.picking = None;
                self.do_promote(nth, hi);
            }
            _ => {}
        }
        true
    }

    /// Queue the add. The local line is only removed once Jira confirms, so a
    /// failed write leaves the item exactly where it was — nothing is ever in
    /// neither place.
    fn do_promote(&mut self, nth: usize, to_group: usize) {
        let Some((gi, ii)) = self.at(nth) else { return };
        let text = self.groups[gi].items[ii].text.clone();
        let Origin::Local { line } = self.groups[gi].items[ii].origin else {
            return;
        };
        let Some(key) = self.groups[to_group].key.clone() else {
            return;
        };
        self.groups[gi].items[ii].dirty = true;
        self.queue(Op::Promote { key: key.clone(), text: text.clone(), line });
        self.status = Some(format!("moving “{text}” → {key}…"));
    }

    // ------------------------------------------------------------- sync ---

    pub fn poll_sync(&mut self) -> bool {
        let Some(syncer) = &self.syncer else {
            return false;
        };
        let done = syncer.poll();
        if done.is_empty() {
            return false;
        }
        let mut failure = None;
        let mut promoted = Vec::new();
        for d in done {
            self.in_flight = self.in_flight.saturating_sub(1);
            match d.error {
                None => {
                    if let Op::Promote { line, .. } = d.op {
                        promoted.push(line);
                    }
                }
                Some(e) => failure = Some(e),
            }
        }
        // Landed promotions can now leave the local file. Descending order so
        // earlier line indices stay valid as we remove.
        if !promoted.is_empty() {
            promoted.sort_unstable_by(|a, b| b.cmp(a));
            if let Some(local) = self.local.as_mut() {
                for line in promoted {
                    local.remove(line);
                }
                if let Err(e) = local.save() {
                    failure = Some(format!("promoted, but todo.md: {e:#}"));
                }
            }
        }
        self.status = Some(match &failure {
            Some(e) => e.clone(),
            None => "synced ✓".into(),
        });
        true
    }

    // ------------------------------------------------------------- keys ---

    /// Handle a key. Editing and picking swallow everything; otherwise this is
    /// the checklist's own keymap.
    pub fn key(&mut self, k: KeyEvent, view_h: u16) -> ListAction {
        if self.editing() && self.edit_key(k) {
            return ListAction::None;
        }
        if self.picking() && self.pick_key(k) {
            return ListAction::None;
        }
        self.status = None;

        let g = std::mem::take(&mut self.pending_g);
        let d = std::mem::take(&mut self.pending_d);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let half = (view_h / 2).max(1) as i32;

        match (k.code, ctrl) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return ListAction::Quit,
            (KeyCode::Char('c'), true) => return ListAction::Quit,

            (KeyCode::Char('j'), false) | (KeyCode::Down, _) => self.move_cursor(1),
            (KeyCode::Char('k'), false) | (KeyCode::Up, _) => self.move_cursor(-1),
            (KeyCode::Char('d'), true) => self.move_cursor(half),
            (KeyCode::Char('u'), true) => self.move_cursor(-half),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => {
                self.move_cursor(view_h as i32)
            }
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => {
                self.move_cursor(-(view_h as i32))
            }
            (KeyCode::Char('g'), false) => {
                if g {
                    self.cursor = 0;
                } else {
                    self.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) | (KeyCode::End, _) => {
                self.cursor = self.item_count().saturating_sub(1)
            }
            (KeyCode::Home, _) => self.cursor = 0,
            (KeyCode::Char('J'), _) => self.move_group(1),
            (KeyCode::Char('K'), _) => self.move_group(-1),

            (KeyCode::Char(' '), _) => self.toggle(),
            (KeyCode::Char('r'), false) => return ListAction::Reload,
            (KeyCode::Char('i'), false) | (KeyCode::Char('A'), _) => {
                self.edit_item(EditMode::Insert)
            }
            (KeyCode::Char('e'), false) => self.edit_item(EditMode::Normal),
            (KeyCode::Char('c'), false) => {
                self.edit_item(EditMode::Normal);
                for _ in 0..2 {
                    self.edit_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
                }
            }
            (KeyCode::Char('o'), false) => self.new_item(),
            (KeyCode::Char('p'), false) => self.promote(),
            (KeyCode::Char('d'), false) => {
                if d {
                    self.delete_item();
                } else {
                    self.pending_d = true;
                }
            }
            (KeyCode::Enter, _) => {
                if let Some((gi, _)) = self.at(self.cursor) {
                    if let Some(key) = self.groups[gi].key.clone() {
                        return ListAction::Open(key);
                    }
                }
            }
            _ => {}
        }
        ListAction::None
    }

    // ------------------------------------------------------------- draw ---

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let view_h = area.height;
        let rows = render::rows(&self.groups);
        let mut lines: Vec<Line> = rows.iter().map(|r| r.line.clone()).collect();

        let editing_row = self.editing.as_ref().and_then(|e| {
            let nth = match e.target {
                Target::Existing(nth) => nth,
                Target::New(gi) => {
                    self.first_of_group(gi) + self.groups.get(gi).map_or(0, |g| g.items.len())
                }
            };
            let row = render::row_of(&rows, nth).or_else(|| {
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
            match lines.get_mut(row) {
                Some(slot) => *slot = line,
                None => lines.push(line),
            }
        }

        // while picking, the highlight moves to the candidate ticket heading
        let th = crate::theme::theme();
        let highlight_row = match self.picking {
            Some((_, gi)) => rows
                .iter()
                .position(|r| r.item.map(|(g, _)| g) == Some(gi))
                .or_else(|| render::row_of(&rows, self.first_of_group(gi))),
            None => editing_row
                .map(|(r, _)| r)
                .or_else(|| render::row_of(&rows, self.cursor)),
        };

        if let Some(row) = highlight_row {
            let row_u = row as u16;
            if row_u < self.scroll {
                self.scroll = row_u;
            } else if view_h > 0 && row_u >= self.scroll + view_h {
                self.scroll = row_u + 1 - view_h;
            }
            if editing_row.is_none() {
                if let Some(l) = lines.get_mut(row) {
                    l.style = l.style.bg(th.selection).fg(th.inverted);
                }
            }
        }
        let max = (lines.len() as u16).saturating_sub(view_h.max(1));
        self.scroll = self.scroll.min(max);

        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            area,
        );
    }

    /// The footer hint for whatever the list is currently doing.
    pub fn hint(&self) -> String {
        let base = if self.editing() {
            " esc discard · ⏎/ZZ save · i insert · vim motions"
        } else if self.picking() {
            " j/k choose ticket · ⏎ move it there · esc cancel"
        } else {
            " j/k move · space done · i edit · o new · dd delete · p promote · ⏎ open · r refresh · q quit"
        };
        match self.in_flight {
            0 => base.to_string(),
            n => format!("{base}  ·  {n} syncing…"),
        }
    }

    pub fn dump(&self) -> Vec<String> {
        render::plain_dump(&self.groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todo::model::TodoItem;
    use ratatui::crossterm::event::KeyEvent;

    fn item(text: &str, origin: Origin) -> TodoItem {
        TodoItem { text: text.into(), done: false, origin, dirty: false }
    }

    fn list() -> ItemList {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![
                    item("a", Origin::Local { line: 0 }),
                    item("b", Origin::Local { line: 1 }),
                ],
            },
            TodoGroup {
                title: "t".into(),
                key: Some("JROZ-1".into()),
                items: vec![item(
                    "c",
                    Origin::Jira { key: "JROZ-1".into(), local_id: "x".into() },
                )],
            },
        ];
        l
    }

    fn press(l: &mut ItemList, c: char) -> ListAction {
        l.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 20)
    }

    #[test]
    fn the_cursor_walks_items_across_group_boundaries() {
        let mut l = list();
        assert_eq!(l.at(0), Some((0, 0)));
        assert_eq!(l.at(2), Some((1, 0)));
        l.move_cursor(9);
        assert_eq!(l.cursor, 2, "clamps at the last item");
        l.move_cursor(-9);
        assert_eq!(l.cursor, 0);
    }

    #[test]
    fn group_jumps_land_on_the_first_item_of_the_group() {
        let mut l = list();
        l.move_group(1);
        assert_eq!(l.cursor, 2);
        l.move_group(-1);
        assert_eq!(l.cursor, 0);
    }

    #[test]
    fn an_empty_list_does_not_panic() {
        let mut l = ItemList::new(None);
        l.move_cursor(1);
        l.move_group(1);
        l.toggle();
        l.delete_item();
        l.new_item();
        l.promote();
        assert_eq!(l.item_count(), 0);
    }

    #[test]
    fn enter_opens_the_ticket_an_item_belongs_to() {
        let mut l = list();
        l.cursor = 2;
        assert_eq!(
            l.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20),
            ListAction::Open("JROZ-1".into())
        );
        // a local item has no ticket to open
        l.cursor = 0;
        assert_eq!(
            l.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20),
            ListAction::None
        );
    }

    #[test]
    fn dd_needs_both_presses() {
        let mut l = list();
        assert!(!l.pending_d);
        press(&mut l, 'd');
        assert!(l.pending_d, "one d only arms the operator");
        // any other key disarms it
        press(&mut l, 'j');
        assert!(!l.pending_d);
    }

    #[test]
    fn editing_swallows_navigation_keys() {
        let mut l = list();
        l.edit_item(EditMode::Insert);
        assert!(l.editing());
        press(&mut l, 'j');
        assert_eq!(l.cursor, 0, "j typed a character, it did not move the cursor");
        assert!(l.editing());
    }

    #[test]
    fn an_unchanged_edit_is_not_written_back() {
        let mut l = list();
        l.cursor = 2; // the Jira item
        l.edit_item(EditMode::Normal);
        // commit without typing
        l.edit_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(l.status.as_deref(), Some("unchanged"));
        assert_eq!(l.in_flight, 0, "nothing must be queued — retext flattens marks");
    }

    #[test]
    fn promotion_only_offers_tickets_and_only_for_local_items() {
        let mut l = list();
        l.cursor = 2;
        l.promote();
        assert!(!l.picking());
        assert_eq!(l.status.as_deref(), Some("already on a ticket"));

        l.cursor = 0;
        l.promote();
        assert!(l.picking(), "a local item can be promoted");

        // with no ticket groups there's nowhere to go
        let mut l = ItemList::new(None);
        l.groups = vec![TodoGroup {
            title: "no ticket".into(),
            key: None,
            items: vec![item("a", Origin::Local { line: 0 })],
        }];
        l.promote();
        assert!(!l.picking());
        assert_eq!(l.status.as_deref(), Some("no tickets to promote into"));
    }

    #[test]
    fn the_picker_walks_only_ticket_groups() {
        let mut l = list();
        l.cursor = 0;
        l.promote();
        assert_eq!(l.picking, Some((0, 1)), "starts on the first ticket group");
        press(&mut l, 'k');
        assert_eq!(l.picking, Some((0, 1)), "cannot walk up into the local group");
        press(&mut l, 'j');
        assert_eq!(l.picking, Some((0, 1)), "only one ticket to choose from");
        l.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 20);
        assert!(!l.picking());
    }
}
