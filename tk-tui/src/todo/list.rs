//! The checklist widget: cursor, modal editing, and the write dispatch.
//!
//! Shared by the aggregate view (many groups, plus the local file) and the
//! ticket pane's TODO mode (one group, Jira only), so the two can't drift.

use super::local::LocalFile;
use super::model::{Origin, TodoGroup};
use super::sync::{Op, Sync};
use crate::editor::{EditMode, Editor, Outcome};
use crate::ui::todo::{self as render, Sel};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

/// What an in-progress edit becomes when it commits.
enum Target {
    Existing(usize),
    /// (group, depth) — a new item inherits the nesting of the one you were
    /// standing on, so `o` under a sub-item keeps you in that sub-list.
    New(usize, usize),
}

struct Edit {
    editor: Editor,
    target: Target,
    /// The buffer as text, refreshed on every keystroke. Kept here so the
    /// renderer can borrow it without the editor handing out a temporary.
    text: String,
}

/// What the host should do after a key the list didn't fully handle.
#[derive(Debug, PartialEq, Eq)]
pub enum ListAction {
    None,
    /// Close the pane entirely.
    Quit,
    /// Step back to wherever you came from, if anywhere.
    Back,
    /// Open the ticket the cursor is on.
    Open(String),
    /// Reload from the backends.
    Reload,
    /// Hand the terminal to fzf and jump to whatever comes back. The list
    /// can't do this itself — the event loop owns the terminal.
    Search,
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
    /// A pending `>` or `<` waiting to be doubled, as in vim.
    pending_shift: Option<char>,
    /// Anchor of a visual-line selection. The selection runs between this and
    /// the cursor, inclusive, the way `V` behaves in vim.
    visual: Option<usize>,
    editing: Option<Edit>,
    /// Choosing a ticket to promote a local item into.
    picking: Option<(usize, usize)>, // (flat item index, highlighted group)
    /// Show only what's left to do. A view setting, nothing more — it lasts
    /// as long as the pane and touches neither backend.
    hide_done: bool,
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
            pending_shift: None,
            visual: None,
            editing: None,
            picking: None,
            hide_done: false,
            syncer: None,
            in_flight: 0,
        }
    }

    /// Writes queued but not yet acknowledged. The footer reads the field
    /// directly; this is for tests that need to wait for a write to land.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.in_flight
    }

    pub fn visual(&self) -> bool {
        self.visual.is_some()
    }

    /// The inclusive range the next operator applies to: the visual selection
    /// if there is one, else just the cursor.
    pub fn range(&self) -> (usize, usize) {
        match self.visual {
            Some(anchor) => (anchor.min(self.cursor), anchor.max(self.cursor)),
            None => (self.cursor, self.cursor),
        }
    }

    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Clamp the cursor after the groups change, keeping your place.
    pub fn clamp(&mut self) {
        let n = self.target_count();
        self.cursor = self.cursor.min(n.saturating_sub(1));
    }

    /// Every position the cursor can occupy, in display order — including one
    /// per empty group, so a ticket with no checkboxes is still reachable.
    fn selectables(&self) -> Vec<Sel> {
        render::selectables(&self.groups, self.hide_done)
    }

    pub fn target_count(&self) -> usize {
        self.selectables().len()
    }

    /// The item under the cursor, if the cursor is on one at all.
    pub fn at(&self, nth: usize) -> Option<(usize, usize)> {
        match self.selectables().get(nth) {
            Some(Sel::Item(gi, ii)) => Some((*gi, *ii)),
            _ => None,
        }
    }

    /// The group under the cursor, whether or not it has any items. This is
    /// what `o` needs: adding to an empty ticket is the whole point.
    pub fn group_at_cursor(&self) -> Option<usize> {
        self.selectables().get(self.cursor).map(|s| s.group())
    }

    fn first_of_group(&self, gi: usize) -> usize {
        self.selectables()
            .iter()
            .position(|s| s.group() == gi)
            .unwrap_or(0)
    }

    fn flat_index_of(&self, origin: &Origin) -> Option<usize> {
        self.selectables().iter().position(|s| match s {
            Sel::Item(gi, ii) => self.groups[*gi].items[*ii].origin == *origin,
            Sel::EmptyGroup(_) => false,
        })
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let n = self.target_count();
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + delta).clamp(0, n as i32 - 1) as usize;
    }

    pub fn move_group(&mut self, dir: i32) {
        let Some(gi) = self.group_at_cursor() else {
            return;
        };
        let target = (gi as i32 + dir).clamp(0, self.groups.len() as i32 - 1) as usize;
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

    /// Show only unfinished items, or everything again.
    ///
    /// The cursor is kept on its item where possible: hiding the done ones is
    /// something you do *while* working down a list, and losing your place
    /// each time would defeat it.
    pub fn toggle_hide_done(&mut self) {
        let was = self.selectables().get(self.cursor).copied();
        self.hide_done = !self.hide_done;
        self.cursor = was
            .and_then(|s| self.selectables().iter().position(|o| *o == s))
            .unwrap_or_else(|| self.cursor.min(self.target_count().saturating_sub(1)));
        self.visual = None;
        self.status = Some(if self.hide_done {
            "showing what's left".into()
        } else {
            "showing everything".into()
        });
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
        // With done items hidden, ticking one takes its row away — the cursor
        // then means the item that moved up into its place, which is the one
        // you want next. It just has to stay in range.
        self.clamp();
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
            text,
        });
    }

    pub fn new_item(&mut self) {
        if self.groups.is_empty() {
            return;
        }
        let Some(gi) = self.group_at_cursor() else {
            return;
        };
        let depth = self
            .at(self.cursor)
            .map(|(g, i)| self.groups[g].items[i].depth)
            .unwrap_or(0);
        self.editing = Some(Edit {
            editor: Editor::new("", EditMode::Insert),
            target: Target::New(gi, depth),
            text: String::new(),
        });
    }

    pub fn edit_key(&mut self, k: KeyEvent) -> bool {
        let Some(edit) = self.editing.as_mut() else {
            return false;
        };
        let outcome = edit.editor.key(k);
        edit.text = edit.editor.text();
        match outcome {
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
                        Target::New(gi, depth) => self.commit_new(gi, depth, &text),
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

    fn commit_new(&mut self, gi: usize, depth: usize, text: &str) {
        // where the cursor is, so the new item lands next to it rather than at
        // the end of the group
        let after = self.at(self.cursor);
        match self.groups[gi].key.clone() {
            None => {
                let after_line = after.and_then(|(g, i)| {
                    match self.groups[g].items[i].origin {
                        Origin::Local { line } if g == gi => Some(line),
                        _ => None,
                    }
                });
                let Some(local) = self.local.as_mut() else { return };
                let line = local.insert_at(text, after_line, Some(depth));
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
                let after_id = after.and_then(|(g, i)| {
                    match &self.groups[g].items[i].origin {
                        Origin::Jira { local_id, .. } if g == gi => Some(local_id.clone()),
                        _ => None,
                    }
                });
                self.queue(Op::Add {
                    key,
                    text: text.to_string(),
                    after: after_id,
                });
                self.status = Some(format!("adding “{text}”…"));
            }
        }
    }

    /// Indent or outdent everything in the current range — one item normally,
    /// or the whole visual selection.
    ///
    /// Top to bottom on purpose: indenting settles each item under its
    /// predecessor before the next one looks for its own, so a run of siblings
    /// ends up in one sub-list rather than a staircase of them.
    pub fn shift_range(&mut self, deeper: bool) {
        let (from, to) = self.range();
        let many = to > from;
        let mut moved = 0;
        let mut refused: Option<String> = None;
        for nth in from..=to {
            match self.shift_one(nth, deeper) {
                Ok(()) => moved += 1,
                Err(why) => {
                    refused.get_or_insert(why);
                }
            }
        }
        self.visual = None;
        self.status = match (moved, refused) {
            (0, Some(why)) => Some(why),
            (0, None) => Some("nothing to indent".into()),
            (n, _) if many => {
                Some(format!("{n} {}", if deeper { "indented" } else { "outdented" }))
            }
            _ => None,
        };
    }

    /// Indent or outdent one item. Both backends can express nesting, so this
    /// is the same gesture whichever one the item lives in.
    fn shift_one(&mut self, nth: usize, deeper: bool) -> Result<(), String> {
        let Some((gi, ii)) = self.at(nth) else {
            return Err("nothing to indent".into());
        };
        let depth = self.groups[gi].items[ii].depth;
        if !deeper && depth == 0 {
            return Err("already at the outer level".into());
        }
        if deeper && (ii == 0 || self.groups[gi].items[ii - 1].depth < depth) {
            return Err("nothing above it to nest under".into());
        }
        match self.groups[gi].items[ii].origin.clone() {
            Origin::Local { line } => {
                let delta = if deeper { 1 } else { -1 };
                let local = self.local.as_mut().ok_or("no local file")?;
                local.shift(line, delta).ok_or("not a checkbox line")?;
                match local.save() {
                    Ok(()) => {
                        self.groups[gi].items[ii].depth = (depth as i32 + delta).max(0) as usize;
                        Ok(())
                    }
                    Err(e) => {
                        self.local.as_mut().expect("checked").shift(line, -delta);
                        Err(format!("{e:#}"))
                    }
                }
            }
            Origin::Jira { key, local_id } => {
                self.groups[gi].items[ii].depth =
                    (depth as i32 + if deeper { 1 } else { -1 }).max(0) as usize;
                self.push_jira(gi, ii, Op::Shift { key, local_id, deeper });
                Ok(())
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
        let shift_op = std::mem::take(&mut self.pending_shift);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let half = (view_h / 2).max(1) as i32;

        // `>`/`<`: one press indents the selection in visual mode, as in vim;
        // doubled, it indents the item under the cursor.
        if let KeyCode::Char(c @ ('>' | '<')) = k.code {
            // a selection fires on the first press; without one it takes two
            if self.visual.is_some() || shift_op == Some(c) {
                self.shift_range(c == '>');
            } else {
                self.pending_shift = Some(c);
            }
            return ListAction::None;
        }

        match (k.code, ctrl) {
            (KeyCode::Char('q'), _) => return ListAction::Quit,
            (KeyCode::Char('c'), true) => return ListAction::Quit,
            // esc leaves the selection before it leaves the view
            (KeyCode::Esc, _) => {
                if self.visual.take().is_some() {
                    return ListAction::None;
                }
                return ListAction::Back;
            }

            // visual-line selection: V anchors, j/k drag it
            (KeyCode::Char('V'), _) | (KeyCode::Char('v'), false) => {
                self.visual = match self.visual {
                    Some(_) => None,
                    None => Some(self.cursor),
                };
            }

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
                self.cursor = self.target_count().saturating_sub(1)
            }
            (KeyCode::Home, _) => self.cursor = 0,
            (KeyCode::Char('J'), _) => self.move_group(1),
            (KeyCode::Char('K'), _) => self.move_group(-1),

            (KeyCode::Char('/'), false) => return ListAction::Search,
            (KeyCode::Char('h'), false) => self.toggle_hide_done(),

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
            // Open works from a group heading as well as from an item, so a
            // ticket with no checkboxes yet is still something you can enter.
            (KeyCode::Enter, _) => {
                if let Some(gi) = self.group_at_cursor() {
                    if let Some(key) = self.groups[gi].key.clone() {
                        return ListAction::Open(key);
                    }
                    self.status = Some("no ticket to open — these are local".into());
                }
            }
            _ => {}
        }
        ListAction::None
    }

    // ----------------------------------------------------------- search ---

    /// One line per cursor position, in display order, for the picker.
    ///
    /// Built from the same `selectables()` the cursor walks, so an index into
    /// this is an index into that by construction — there is no second
    /// ordering to keep in step.
    ///
    /// The ticket key goes in the line rather than in a heading, so fzf's
    /// space-separated AND does the work a grouped list would otherwise need
    /// special handling for: `jroz-2 cache` means what you'd expect.
    pub fn search_lines(&self) -> Vec<String> {
        let sels = self.selectables();
        let label = |gi: usize| -> String {
            match &self.groups[gi].key {
                Some(k) => k.clone(),
                None => "todo.md".to_string(),
            }
        };
        let width = sels
            .iter()
            .map(|s| label(s.group()).chars().count())
            .max()
            .unwrap_or(0);
        sels.iter()
            .map(|s| match s {
                Sel::Item(gi, ii) => {
                    let it = &self.groups[*gi].items[*ii];
                    format!(
                        "{:width$}  {} {}{}",
                        label(*gi),
                        if it.done { '☑' } else { '☐' },
                        "  ".repeat(it.depth),
                        it.text
                    )
                }
                Sel::EmptyGroup(gi) => {
                    format!("{:width$}  · nothing yet", label(*gi))
                }
            })
            .collect()
    }

    /// Put the cursor on the nth position. Out of range is ignored rather
    /// than clamped: a picker that returned nonsense should move nothing.
    pub fn search_pick(&mut self, nth: usize) {
        if nth < self.target_count() {
            self.cursor = nth;
            self.visual = None;
        }
    }

    // ------------------------------------------------------------- draw ---

    /// Describe the in-progress edit to the renderer, so the buffer is drawn
    /// where the item actually is instead of patched over a row afterwards.
    fn editing_for_render(&self) -> render::Editing<'_> {
        let Some(e) = &self.editing else {
            return render::Editing::None;
        };
        let insert = e.editor.mode() == EditMode::Insert;
        let cursor = e.editor.cursor();
        match e.target {
            Target::Existing(nth) => match self.at(nth) {
                Some((gi, ii)) => render::Editing::Existing {
                    gi,
                    ii,
                    text: &e.text,
                    cursor,
                    insert,
                },
                None => render::Editing::None,
            },
            Target::New(gi, depth) => render::Editing::New {
                gi,
                text: &e.text,
                cursor,
                insert,
                depth,
            },
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let view_h = area.height;
        let rows = render::rows(&self.groups, self.editing_for_render(), self.hide_done);
        let mut lines: Vec<Line> = rows.iter().map(|r| r.line.clone()).collect();

        // While editing, the caret is the highlight; while picking, the
        // highlight moves to the candidate ticket; otherwise it's the cursor.
        let th = crate::theme::theme();
        let editing_row = render::editing_row(&rows);
        let highlight_row = match (editing_row, self.picking) {
            (Some(r), _) => Some(r),
            (None, Some((_, gi))) => rows
                .iter()
                .position(|r| r.sel.map(Sel::group) == Some(gi))
                .or_else(|| render::row_of(&rows, self.first_of_group(gi))),
            (None, None) => render::row_of(&rows, self.cursor),
        };

        if let Some(row) = highlight_row {
            let row_u = row as u16;
            if row_u < self.scroll {
                self.scroll = row_u;
            } else if view_h > 0 && row_u >= self.scroll + view_h {
                self.scroll = row_u + 1 - view_h;
            }
            if editing_row.is_none() {
                // the whole visual range is painted, not just the cursor row
                let (from, to) = if self.picking.is_some() {
                    (self.cursor, self.cursor)
                } else {
                    self.range()
                };
                let painted = if self.picking.is_some() {
                    vec![row]
                } else {
                    (from..=to).filter_map(|n| render::row_of(&rows, n)).collect()
                };
                for r in painted {
                    if let Some(l) = lines.get_mut(r) {
                        l.style = l.style.bg(th.selection).fg(th.inverted);
                    }
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
        } else if self.visual() {
            " ── VISUAL ── j/k extend · > indent · < outdent · esc cancel"
        } else {
            // The state marker leads: this footer is longer than a floating
            // pane is wide, and the tail is the first thing to go.
            return self.keymap_hint();
        };
        let base = base.to_string();
        match self.in_flight {
            0 => base,
            n => format!("{base}  ·  {n} syncing…"),
        }
    }

    /// The ordinary keymap line. The state marker leads when done items are
    /// hidden, because this footer is longer than a floating pane is wide and
    /// the tail is the first thing to go.
    fn keymap_hint(&self) -> String {
        let line = if self.hide_done {
            " ── open only ── h all · j/k move · space done · i edit · o new · / find \
· >>/<< indent · V select · dd delete · p promote · ⏎ open · q quit"
        } else {
            " j/k move · space done · i edit · o new · / find · h hide done \
· >>/<< indent · V select · dd delete · p promote · ⏎ open · q quit"
        };
        match self.in_flight {
            0 => line.to_string(),
            n => format!("{line}  ·  {n} syncing…"),
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
        TodoItem { text: text.into(), done: false, origin, dirty: false, depth: 0 }
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

    /// The bug from the screenshot: two ticket groups with no checkboxes yet
    /// were unreachable, so `j` stopped at the last local item and `o` always
    /// fell back to group 0 — you could only ever add to "no ticket".
    #[test]
    fn the_cursor_reaches_a_ticket_that_has_no_items_yet() {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("a", Origin::Local { line: 0 })],
            },
            TodoGroup { title: "t2".into(), key: Some("JROZ-2".into()), items: vec![] },
            TodoGroup { title: "t1".into(), key: Some("JROZ-1".into()), items: vec![] },
        ];

        assert_eq!(l.target_count(), 3, "each empty ticket is a position");
        assert_eq!(l.group_at_cursor(), Some(0));

        press(&mut l, 'j');
        assert_eq!(l.group_at_cursor(), Some(1), "j reaches the empty JROZ-2");
        assert_eq!(l.at(l.cursor), None, "…and it is not an item");

        press(&mut l, 'j');
        assert_eq!(l.group_at_cursor(), Some(2));
        press(&mut l, 'j');
        assert_eq!(l.group_at_cursor(), Some(2), "clamps at the end");
        press(&mut l, 'k');
        assert_eq!(l.group_at_cursor(), Some(1));
    }

    #[test]
    fn o_on_an_empty_ticket_adds_to_that_ticket_not_to_the_local_file() {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("a", Origin::Local { line: 0 })],
            },
            TodoGroup { title: "t".into(), key: Some("JROZ-2".into()), items: vec![] },
        ];

        press(&mut l, 'j');
        press(&mut l, 'o');
        for c in "wire the harness".chars() {
            press(&mut l, c);
        }
        l.edit_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(l.in_flight, 1, "the add must be queued for Jira");
        assert!(
            l.status.as_deref().unwrap_or("").contains("wire the harness"),
            "got: {:?}",
            l.status
        );
    }

    #[test]
    fn shift_j_and_k_jump_to_empty_groups_too() {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("a", Origin::Local { line: 0 })],
            },
            TodoGroup { title: "t".into(), key: Some("JROZ-2".into()), items: vec![] },
        ];
        press(&mut l, 'J');
        assert_eq!(l.group_at_cursor(), Some(1));
        press(&mut l, 'K');
        assert_eq!(l.group_at_cursor(), Some(0));
    }

    /// Operations that need a real item must decline politely on an empty
    /// group rather than acting on whatever happens to be at index 0.
    #[test]
    fn item_operations_are_no_ops_on_an_empty_group() {
        let mut l = ItemList::new(None);
        l.groups = vec![TodoGroup {
            title: "t".into(),
            key: Some("JROZ-2".into()),
            items: vec![],
        }];
        l.toggle();
        l.delete_item();
        l.promote();
        assert_eq!(l.in_flight, 0, "nothing may be written");
    }

    fn three_local() -> ItemList {
        let mut l = ItemList::new(None);
        l.groups = vec![TodoGroup {
            title: "no ticket".into(),
            key: None,
            items: vec![
                item("first", Origin::Local { line: 0 }),
                item("second", Origin::Local { line: 1 }),
                item("third", Origin::Local { line: 2 }),
            ],
        }];
        l
    }

    /// vim: one `>` is a pending operator, the second one fires it.
    #[test]
    fn a_single_angle_bracket_waits_to_be_doubled() {
        let mut l = three_local();
        l.cursor = 1;
        press(&mut l, '>');
        assert_eq!(l.pending_shift, Some('>'), "armed, not fired");
        assert_eq!(l.groups[0].items[1].depth, 0, "nothing has moved yet");

        // an unrelated key disarms it, as in vim
        press(&mut l, 'j');
        assert_eq!(l.pending_shift, None);
    }

    #[test]
    fn mismatched_brackets_do_not_fire() {
        let mut l = three_local();
        l.cursor = 1;
        press(&mut l, '>');
        press(&mut l, '<');
        assert_eq!(l.pending_shift, Some('<'), "the second arms its own operator");
        assert_eq!(l.groups[0].items[1].depth, 0);
    }

    #[test]
    fn v_anchors_a_selection_that_j_and_k_drag() {
        let mut l = three_local();
        assert_eq!(l.range(), (0, 0), "no selection is just the cursor");

        press(&mut l, 'V');
        assert!(l.visual());
        press(&mut l, 'j');
        assert_eq!(l.range(), (0, 1));
        press(&mut l, 'j');
        assert_eq!(l.range(), (0, 2));
        press(&mut l, 'k');
        assert_eq!(l.range(), (0, 1));

        // dragging upward from the anchor works too
        let mut l = three_local();
        l.cursor = 2;
        press(&mut l, 'V');
        press(&mut l, 'k');
        assert_eq!(l.range(), (1, 2), "the range is ordered, whichever way you drag");
    }

    #[test]
    fn esc_drops_the_selection_before_it_leaves_the_view() {
        let mut l = three_local();
        press(&mut l, 'V');
        let act = l.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 20);
        assert_eq!(act, ListAction::None, "the first esc only cancels the selection");
        assert!(!l.visual());
        let act = l.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 20);
        assert_eq!(act, ListAction::Back, "the second leaves");
    }

    /// In visual mode a single `>` fires, exactly as vim does.
    #[test]
    fn one_angle_bracket_shifts_a_whole_selection() {
        let mut l = three_local();
        l.local = Some(
            LocalFile::load_from(std::env::temp_dir().join(format!(
                "tk-visual-{}.md",
                std::process::id()
            )))
            .unwrap(),
        );
        // give the backing file matching lines so the shift can save
        std::fs::write(
            std::env::temp_dir().join(format!("tk-visual-{}.md", std::process::id())),
            "- [ ] first\n- [ ] second\n- [ ] third\n",
        )
        .unwrap();
        l.local = Some(
            LocalFile::load_from(std::env::temp_dir().join(format!(
                "tk-visual-{}.md",
                std::process::id()
            )))
            .unwrap(),
        );

        l.cursor = 1;
        press(&mut l, 'V');
        press(&mut l, 'j');
        assert_eq!(l.range(), (1, 2));
        press(&mut l, '>');

        assert!(!l.visual(), "the selection clears after the operator, as in vim");
        assert_eq!(l.groups[0].items[1].depth, 1);
        assert_eq!(l.groups[0].items[2].depth, 1, "both moved, not just the cursor");
        assert_eq!(l.groups[0].items[0].depth, 0, "and nothing outside the range");
        assert_eq!(l.status.as_deref(), Some("2 indented"));

        std::fs::remove_file(
            std::env::temp_dir().join(format!("tk-visual-{}.md", std::process::id())),
        )
        .ok();
    }

    #[test]
    fn the_first_item_refuses_to_indent_and_says_why() {
        let mut l = three_local();
        press(&mut l, '>');
        press(&mut l, '>');
        assert_eq!(l.groups[0].items[0].depth, 0);
        assert_eq!(l.status.as_deref(), Some("nothing above it to nest under"));
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
        assert_eq!(l.target_count(), 0);
    }

    #[test]
    fn enter_opens_a_ticket_that_has_no_items_yet() {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("a", Origin::Local { line: 0 })],
            },
            TodoGroup { title: "t".into(), key: Some("JROZ-2".into()), items: vec![] },
        ];
        press(&mut l, 'j');
        assert_eq!(
            l.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20),
            ListAction::Open("JROZ-2".into())
        );
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

/// Render tests. These drive a real terminal buffer rather than the plain-text
/// dump, because every bug in the first cut of this pane was a rendering bug
/// the dump could not see: the edit buffer drawn nowhere, and the highlight
/// never checked at all.
#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn item(text: &str, line: usize) -> crate::todo::model::TodoItem {
        crate::todo::model::TodoItem {
            text: text.into(),
            done: false,
            origin: Origin::Local { line },
            dirty: false,
            depth: 0,
        }
    }

    fn populated() -> ItemList {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("first local", 0), item("second local", 1)],
            },
            TodoGroup {
                title: "a ticket".into(),
                key: Some("JROZ-1".into()),
                items: vec![item("on the ticket", 0)],
            },
        ];
        l
    }

    fn empty() -> ItemList {
        let mut l = ItemList::new(None);
        l.groups = vec![TodoGroup {
            title: "no ticket".into(),
            key: None,
            items: vec![],
        }];
        l
    }

    /// Everything the terminal actually shows, one string per row.
    fn render(l: &mut ItemList) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
        term.draw(|f| {
            let area = f.area();
            l.draw(f, area);
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// The row painted with the selection background.
    fn highlighted(l: &mut ItemList) -> Option<String> {
        let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
        term.draw(|f| {
            let area = f.area();
            l.draw(f, area);
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        let sel = crate::theme::theme().selection;
        (0..buf.area.height).find_map(|y| {
            (buf[(0, y)].style().bg == Some(sel)).then(|| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
        })
    }

    #[test]
    fn j_and_k_visibly_move_the_highlight() {
        let mut l = populated();
        assert!(
            highlighted(&mut l).unwrap().contains("first local"),
            "the first item starts highlighted"
        );

        l.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), 14);
        assert!(highlighted(&mut l).unwrap().contains("second local"));

        // and across a group boundary
        l.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), 14);
        assert!(highlighted(&mut l).unwrap().contains("on the ticket"));

        l.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE), 14);
        assert!(highlighted(&mut l).unwrap().contains("second local"));
    }

    /// The one that mattered: `o` on an empty list used to render nothing at
    /// all, so you typed blind.
    #[test]
    fn o_on_an_empty_list_shows_what_you_are_typing() {
        let mut l = empty();
        l.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE), 14);
        for c in "buy milk".chars() {
            l.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 14);
        }
        let screen = render(&mut l).join("\n");
        assert!(
            screen.contains("buy milk"),
            "the buffer must be on screen while typing; got:\n{screen}"
        );
        assert!(
            !screen.contains("nothing yet"),
            "the placeholder must give way to the new item"
        );
    }

    #[test]
    fn a_new_item_is_typed_at_the_end_of_its_own_group() {
        let mut l = populated();
        l.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE), 14);
        for c in "third".chars() {
            l.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 14);
        }
        let screen = render(&mut l);
        let at = screen.iter().position(|r| r.contains("third")).expect("on screen");
        let second = screen.iter().position(|r| r.contains("second local")).unwrap();
        let ticket = screen.iter().position(|r| r.contains("JROZ-1")).unwrap();
        assert!(
            second < at && at < ticket,
            "the new row belongs after its group's last item and before the next group:\n{screen:#?}"
        );
        // and it must not have eaten an existing row
        assert!(screen.iter().any(|r| r.contains("on the ticket")));
    }

    #[test]
    fn editing_an_item_shows_the_buffer_in_that_items_place() {
        let mut l = populated();
        l.cursor = 1;
        l.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), 14);
        for c in "!!".chars() {
            l.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 14);
        }
        let screen = render(&mut l);
        let at = screen
            .iter()
            .position(|r| r.contains("second local!!"))
            .expect("the edited text is on screen");
        let first = screen.iter().position(|r| r.contains("first local")).unwrap();
        assert_eq!(at, first + 1, "it stays in its own row");
        assert!(screen.iter().any(|r| r.contains("on the ticket")), "nothing else moved");
    }

    /// The cursor indexes one list and the renderer paints another; if they
    /// ever disagree the highlight lands on the wrong row. Hold them in step.
    #[test]
    fn every_cursor_position_has_exactly_one_row() {
        let mut l = populated();
        l.groups.push(TodoGroup {
            title: "empty".into(),
            key: Some("JROZ-9".into()),
            items: vec![],
        });
        let rows = render::rows(&l.groups, render::Editing::None, false);
        let painted: Vec<_> = rows.iter().filter_map(|r| r.sel).collect();
        assert_eq!(painted, l.selectables());
        assert_eq!(painted.len(), l.target_count());
        for n in 0..l.target_count() {
            assert!(render::row_of(&rows, n).is_some(), "position {n} has no row");
        }
    }

    #[test]
    fn the_highlight_can_land_on_an_empty_group() {
        let mut l = populated();
        l.groups.push(TodoGroup {
            title: "empty ticket".into(),
            key: Some("JROZ-9".into()),
            items: vec![],
        });
        l.cursor = l.target_count() - 1;
        assert!(
            highlighted(&mut l).unwrap().contains("nothing yet"),
            "the placeholder row is where the cursor sits"
        );
    }

    /// A selection you can't see is a selection you can't trust.
    #[test]
    fn the_whole_visual_selection_is_painted() {
        let mut l = populated();
        let sel = crate::theme::theme().selection;
        let rows_bg = |l: &mut ItemList| {
            let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
            term.draw(|f| {
                let area = f.area();
                l.draw(f, area);
            })
            .unwrap();
            let buf = term.backend().buffer().clone();
            (0..buf.area.height)
                .filter(|y| buf[(0, *y)].style().bg == Some(sel))
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(rows_bg(&mut l).len(), 1, "just the cursor to start with");

        l.key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::NONE), 14);
        l.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), 14);
        let painted = rows_bg(&mut l);
        assert_eq!(painted.len(), 2, "both rows of the selection: {painted:?}");
        assert!(painted[0].contains("first local"));
        assert!(painted[1].contains("second local"));
    }

    #[test]
    fn nesting_is_visible_on_screen() {
        let mut l = ItemList::new(None);
        let mk = |t: &str, d: usize| {
            let mut i = item(t, 0);
            i.depth = d;
            i
        };
        l.groups = vec![TodoGroup {
            title: "no ticket".into(),
            key: None,
            items: vec![mk("top", 0), mk("child", 1), mk("grandchild", 2)],
        }];
        let screen = render(&mut l);
        let col = |needle: &str| {
            screen
                .iter()
                .find(|r| r.contains(needle))
                .map(|r| r.chars().position(|c| c == '☐').unwrap())
                .unwrap()
        };
        assert!(col("child") > col("top"), "a child is indented past its parent");
        assert!(col("grandchild") > col("child"), "and so on down");
    }

    #[test]
    fn a_new_item_is_typed_at_the_depth_it_will_have() {
        let mut l = ItemList::new(None);
        let mut nested = item("child", 1);
        nested.depth = 1;
        l.groups = vec![TodoGroup {
            title: "no ticket".into(),
            key: None,
            items: vec![item("top", 0), nested],
        }];
        l.cursor = 1;
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        l.key(key('o'), 14);
        for c in "sibling".chars() {
            l.key(key(c), 14);
        }
        let screen = render(&mut l);
        // columns, not bytes: the edit marker ❯ is three bytes and one column
        let col = |needle: &str| {
            screen
                .iter()
                .find(|r| r.contains(needle))
                .map(|r| r.chars().position(|c| c == '☐').unwrap())
        };
        assert_eq!(
            col("sibling"),
            col("child"),
            "the buffer sits where the item will, not at the margin"
        );
    }

    #[test]
    fn the_checklist_renders_its_groups_and_boxes() {
        let mut l = populated();
        let screen = render(&mut l);
        assert!(screen.iter().any(|r| r.contains("no ticket")));
        assert!(screen.iter().any(|r| r.contains("JROZ-1")));
        assert!(screen.iter().filter(|r| r.contains('☐')).count() == 3);
    }
}

/// The `/` handover. The picker itself is tested in `pick.rs`; what matters
/// here is that the lines it's given and the positions the cursor can occupy
/// are the *same sequence* — an index from one has to mean the same thing in
/// the other, or `⏎` in fzf lands you on the wrong todo.
#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::todo::model::TodoItem;

    fn item(text: &str, done: bool, depth: usize) -> TodoItem {
        TodoItem {
            text: text.into(),
            done,
            origin: Origin::Local { line: 0 },
            dirty: false,
            depth,
        }
    }

    fn list() -> ItemList {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("buy milk", false, 0)],
            },
            TodoGroup {
                title: "Get FraudGen ready".into(),
                key: Some("JROZ-2".into()),
                items: vec![
                    item("wire up retry", false, 0),
                    item("the nested one", true, 1),
                ],
            },
            // a ticket with nothing on it yet
            TodoGroup {
                title: "Later".into(),
                key: Some("JROZ-10".into()),
                items: vec![],
            },
        ];
        l
    }

    #[test]
    fn there_is_exactly_one_line_per_cursor_position() {
        let l = list();
        assert_eq!(l.search_lines().len(), l.target_count());
        assert_eq!(l.search_lines().len(), 4, "3 items + the empty ticket");
    }

    /// The property the whole feature rests on.
    #[test]
    fn line_n_is_the_item_the_cursor_reaches_at_n() {
        let mut l = list();
        for (n, line) in l.search_lines().iter().enumerate() {
            l.search_pick(n);
            assert_eq!(l.cursor, n);
            match l.at(n) {
                Some((gi, ii)) => assert!(
                    line.contains(&l.groups[gi].items[ii].text),
                    "line {n} ({line:?}) is not the item the cursor landed on"
                ),
                None => assert!(
                    line.contains("nothing yet"),
                    "line {n} ({line:?}) should be the empty ticket"
                ),
            }
        }
    }

    #[test]
    fn each_line_carries_its_ticket_so_fzf_can_and_the_terms() {
        let l = list();
        let lines = l.search_lines();
        // `jroz-2 retry` in fzf is two AND-ed terms; both have to be present
        assert!(lines[1].contains("JROZ-2") && lines[1].contains("wire up retry"));
        // local items are searchable by where they live too
        assert!(lines[0].contains("todo.md") && lines[0].contains("buy milk"));
    }

    #[test]
    fn state_and_nesting_are_visible_in_the_picker() {
        let lines = list().search_lines();
        assert!(lines[1].contains('☐'), "open: {:?}", lines[1]);
        assert!(lines[2].contains('☑'), "done: {:?}", lines[2]);
        assert!(
            lines[2].contains("  the nested one"),
            "depth 1 indents: {:?}",
            lines[2]
        );
    }

    /// An empty ticket is a cursor position, so it must be reachable from the
    /// picker as well — jumping there and pressing `o` is the point of it.
    #[test]
    fn a_ticket_with_no_items_can_still_be_jumped_to() {
        let mut l = list();
        let lines = l.search_lines();
        assert!(lines[3].contains("JROZ-10"), "got {:?}", lines[3]);
        l.search_pick(3);
        assert_eq!(l.group_at_cursor(), Some(2));
        assert_eq!(l.at(3), None, "it's a group, not an item");
    }

    #[test]
    fn a_nonsense_index_moves_nothing() {
        let mut l = list();
        l.search_pick(2);
        l.search_pick(99);
        assert_eq!(l.cursor, 2);
    }

    /// A pick out of a visual selection would leave the selection stretched
    /// across wherever you jumped from, which is never what you meant.
    #[test]
    fn jumping_drops_a_visual_selection() {
        let mut l = list();
        l.key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::NONE), 14);
        assert!(l.visual());
        l.search_pick(2);
        assert!(!l.visual());
    }

    #[test]
    fn slash_asks_for_the_picker_and_the_footer_says_so() {
        let mut l = list();
        let act = l.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 14);
        assert_eq!(act, ListAction::Search);
        assert!(l.hint().contains("/ find"), "got {:?}", l.hint());
    }
}

/// `h` — show only what's left. A view setting, so the same rule as the
/// picker applies: what's drawn and what the cursor can address are one
/// sequence, and neither backend hears about it.
#[cfg(test)]
mod hide_done_tests {
    use super::*;
    use crate::todo::model::TodoItem;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn item(text: &str, done: bool, depth: usize) -> TodoItem {
        TodoItem {
            text: text.into(),
            done,
            origin: Origin::Local { line: 0 },
            dirty: false,
            depth,
        }
    }

    fn list() -> ItemList {
        let mut l = ItemList::new(None);
        l.groups = vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![
                    item("buy milk", false, 0),
                    item("call the vet", true, 0),
                    item("walk the dog", false, 0),
                ],
            },
            TodoGroup {
                title: "all finished".into(),
                key: Some("JROZ-9".into()),
                items: vec![item("shipped it", true, 0)],
            },
        ];
        l
    }

    fn press(l: &mut ItemList, c: char) {
        l.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 14);
    }

    fn screen(l: &mut ItemList) -> String {
        let mut term = Terminal::new(TestBackend::new(60, 16)).unwrap();
        term.draw(|f| {
            let area = f.area();
            l.draw(f, area);
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn h_hides_the_done_ones_and_h_again_brings_them_back() {
        let mut l = list();
        assert!(screen(&mut l).contains("call the vet"));

        press(&mut l, 'h');
        let s = screen(&mut l);
        assert!(!s.contains("call the vet"), "the done one is gone:\n{s}");
        assert!(s.contains("buy milk") && s.contains("walk the dog"));

        press(&mut l, 'h');
        assert!(screen(&mut l).contains("call the vet"));
    }

    #[test]
    fn the_cursor_cannot_reach_what_is_hidden() {
        let mut l = list();
        assert_eq!(l.target_count(), 4);
        press(&mut l, 'h');
        assert_eq!(l.target_count(), 3, "2 open + the finished group's heading");
        for n in 0..l.target_count() {
            if let Some((gi, ii)) = l.at(n) {
                assert!(!l.groups[gi].items[ii].done, "position {n} is a done item");
            }
        }
    }

    /// You press `h` in the middle of working down a list; losing your place
    /// every time would defeat the point of it.
    #[test]
    fn your_place_is_kept_across_the_toggle() {
        let mut l = list();
        press(&mut l, 'j');
        press(&mut l, 'j'); // "walk the dog", past the done one
        assert_eq!(l.at(l.cursor).map(|(g, i)| l.groups[g].items[i].text.clone()),
                   Some("walk the dog".into()));
        press(&mut l, 'h');
        assert_eq!(l.at(l.cursor).map(|(g, i)| l.groups[g].items[i].text.clone()),
                   Some("walk the dog".into()));
        press(&mut l, 'h');
        assert_eq!(l.at(l.cursor).map(|(g, i)| l.groups[g].items[i].text.clone()),
                   Some("walk the dog".into()));
    }

    /// Standing on a done item when it's hidden — there is nowhere to keep
    /// your place, so the cursor just has to stay somewhere real.
    #[test]
    fn hiding_the_item_you_are_standing_on_leaves_the_cursor_valid() {
        let mut l = list();
        press(&mut l, 'j'); // "call the vet", which is done
        press(&mut l, 'h');
        assert!(l.cursor < l.target_count());
        if let Some((gi, ii)) = l.at(l.cursor) {
            assert!(!l.groups[gi].items[ii].done);
        }
    }

    /// A group can go empty without being empty, and saying "nothing yet"
    /// there would be a lie — the work is done, not missing.
    #[test]
    fn a_fully_finished_ticket_says_so_and_stays_reachable() {
        let mut l = list();
        press(&mut l, 'h');
        let s = screen(&mut l);
        assert!(s.contains("JROZ-9"), "the ticket is still listed:\n{s}");
        assert!(s.contains("all done"), "and says why it's empty:\n{s}");
        assert!(!s.contains("nothing yet"), "which is a different thing:\n{s}");

        // still a cursor position, so `o` can add to it
        l.cursor = l.target_count() - 1;
        assert_eq!(l.group_at_cursor(), Some(1));
    }

    /// Ticking a parent isn't a claim about the work nested under it.
    #[test]
    fn a_done_parent_with_unfinished_children_stays_visible() {
        let mut l = list();
        l.groups = vec![TodoGroup {
            title: "no ticket".into(),
            key: None,
            items: vec![
                item("the epic", true, 0),
                item("still to do", false, 1),
                item("finished bit", true, 1),
                item("unrelated, done", true, 0),
            ],
        }];
        press(&mut l, 'h');
        let s = screen(&mut l);
        assert!(s.contains("the epic"), "the parent stays:\n{s}");
        assert!(s.contains("still to do"));
        assert!(!s.contains("finished bit"), "its done child goes:\n{s}");
        assert!(!s.contains("unrelated, done"), "and so does a done leaf:\n{s}");
    }

    /// The picker shows what the list shows — jumping to an invisible item
    /// would be a way to end up somewhere the cursor can't be.
    #[test]
    fn the_fzf_lines_follow_the_toggle() {
        let mut l = list();
        assert_eq!(l.search_lines().len(), 4);
        press(&mut l, 'h');
        let lines = l.search_lines();
        assert_eq!(lines.len(), l.target_count());
        assert!(!lines.iter().any(|s| s.contains("call the vet")));
    }

    /// Ticking the last open item makes its row vanish under `h`. The cursor
    /// has to land somewhere real, not one past the end.
    #[test]
    fn ticking_the_last_open_item_does_not_strand_the_cursor() {
        let path = std::env::temp_dir().join(format!("tk-hide-{}.md", std::process::id()));
        std::fs::write(&path, "- [ ] buy milk\n- [x] call the vet\n- [ ] walk the dog\n").unwrap();
        let mut l = ItemList::new(Some(LocalFile::load_from(path.clone()).unwrap()));
        l.groups = vec![l.local.as_ref().unwrap().group()];

        press(&mut l, 'h');
        l.cursor = l.target_count() - 1; // the last item still showing
        press(&mut l, ' ');

        assert!(l.cursor < l.target_count().max(1), "cursor {} of {}", l.cursor, l.target_count());
        assert!(!screen(&mut l).contains("walk the dog"), "it ticked and went");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn the_footer_says_which_way_h_points() {
        let mut l = list();
        assert!(l.hint().contains("h hide done"), "got {:?}", l.hint());
        press(&mut l, 'h');
        assert!(l.hint().contains("open only"), "got {:?}", l.hint());
        assert!(l.hint().contains("h all"), "got {:?}", l.hint());
    }
}
