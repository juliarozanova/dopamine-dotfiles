//! App state + the actions that mutate it. Terminal/event plumbing lives in
//! main.rs; rendering in ui.rs.

use crate::editor::{self, EditMode, Editor};
use crate::jira::{self, Ticket};
use crate::todo::list::{ItemList, ListAction};
use crate::todo::model::TodoGroup;
use crate::ui::{build, line_text, Segments};
use crate::view::Action;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Mode {
    Normal,
    /// Composing a comment; `reply_to` is an index into ticket.comments.
    Compose { reply_to: Option<usize> },
}

/// What the pane is showing. `t` toggles.
///
/// Deliberately a mode rather than a second pane in the ticket layout: the
/// description already contains the TODO section, so a separate pane would be
/// a strict subset of its neighbour — the redundancy that got the hdl inbox
/// removed in the first place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Full,
    Todo,
}

pub struct App {
    pub key: String,
    pub ticket: Ticket,
    pub segs: Segments,
    pub scroll: u16,
    pub sel: Option<usize>, // selected comment
    pub mode: Mode,
    /// The compose buffer. Same modal editor the checklist uses, so `ctrl-s`
    /// is no longer needed to send — which matters, because `ctrl-s` is
    /// terminal flow control and the terminal eats it before we see it.
    pub input: Editor,
    pub status: Option<String>,
    pub pending_g: bool,
    pub focus: Focus,
    /// True when this pane was opened from the checklist, so `esc` has
    /// somewhere to go back to and the footer can say so.
    pub nested: bool,
    /// This ticket's TODO section, as an editable checklist. Rebuilt whenever
    /// the ticket is (re)loaded.
    pub todo: ItemList,
}

impl App {
    pub fn new(key: &str) -> Result<Self> {
        let ticket = jira::fetch(key)?;
        let segs = build(&ticket);
        let mut app = Self {
            key: key.to_string(),
            ticket,
            segs,
            scroll: 0,
            sel: None,
            mode: Mode::Normal,
            input: Editor::new("", EditMode::Insert),
            status: None,
            pending_g: false,
            focus: Focus::Full,
            nested: false,
            todo: ItemList::new(None),
        };
        app.rebuild_todo();
        Ok(app)
    }

    fn rebuild_todo(&mut self) {
        let items = self
            .ticket
            .description
            .as_ref()
            .map(|d| crate::todo::jira::items(&self.key, d))
            .unwrap_or_default();
        self.todo.groups = vec![TodoGroup {
            title: self.ticket.summary.clone(),
            key: Some(self.key.clone()),
            items,
        }];
        self.todo.clamp();
    }

    pub fn tick(&mut self) {
        if self.todo.poll_sync() {
            let msg = self.todo.status.clone();
            self.reload();
            self.todo.status = msg;
        }
    }

    pub fn reload(&mut self) {
        match jira::fetch(&self.key) {
            Ok(t) => {
                self.ticket = t;
                self.segs = build(&self.ticket);
                let n = self.segs.comment_headers.len();
                if let Some(s) = self.sel {
                    if s >= n {
                        self.sel = n.checked_sub(1);
                    }
                }
                self.rebuild_todo();
                self.status = Some("refreshed".into());
            }
            Err(e) => self.status = Some(format!("refresh failed: {e:#}")),
        }
    }

    pub fn max_scroll(&self, view_h: u16) -> u16 {
        (self.segs.lines.len() as u16).saturating_sub(view_h.max(1))
    }

    pub fn scroll_by(&mut self, delta: i32, view_h: u16) {
        let max = self.max_scroll(view_h) as i32;
        self.scroll = (self.scroll as i32 + delta).clamp(0, max) as u16;
    }

    /// Move comment selection by ±1 and bring its header into view.
    pub fn select_comment(&mut self, dir: i32, view_h: u16) {
        let n = self.segs.comment_headers.len();
        if n == 0 {
            self.status = Some("no comments".into());
            return;
        }
        let next = match self.sel {
            None => {
                if dir >= 0 {
                    0
                } else {
                    n - 1
                }
            }
            Some(s) => (s as i32 + dir).clamp(0, n as i32 - 1) as usize,
        };
        self.sel = Some(next);
        let header = self.segs.comment_headers[next] as u16;
        let max = self.max_scroll(view_h);
        // scroll so the header sits near the top third of the viewport
        self.scroll = header.saturating_sub(view_h / 3).min(max);
    }

    /// Body for a reply: quote the selected comment, then the user's text.
    fn reply_body(&self, reply_to: usize, text: &str) -> String {
        let c = &self.ticket.comments[reply_to];
        let mut out = format!("> {} wrote:\n", c.author);
        let quoted: Vec<String> = crate::adf::to_lines(&c.body)
            .iter()
            .map(line_text)
            .collect();
        for (i, l) in quoted.iter().enumerate() {
            if i >= 8 {
                out.push_str("> …\n");
                break;
            }
            out.push_str("> ");
            out.push_str(l);
            out.push('\n');
        }
        out.push('\n');
        out.push_str(text);
        out
    }

    pub fn submit(&mut self) {
        let text = self.input.text().trim().to_string();
        if text.is_empty() {
            self.status = Some("empty — not sent".into());
            self.mode = Mode::Normal;
            return;
        }
        let body = match self.mode {
            Mode::Compose {
                reply_to: Some(i),
            } => self.reply_body(i, &text),
            _ => text,
        };
        match jira::add_comment(&self.key, &body) {
            Ok(()) => {
                self.input = Editor::new("", EditMode::Insert);
                self.mode = Mode::Normal;
                self.reload();
                self.status = Some("comment posted ✓".into());
            }
            // stay in compose so the draft isn't lost
            Err(e) => self.status = Some(format!("post failed: {e:#}")),
        }
    }
    /// The ticket pane's keymap. In TODO mode the checklist owns most keys;
    /// `t` always comes back here.
    pub fn key(&mut self, k: KeyEvent, view_h: u16) -> Action {
        if let Mode::Compose { .. } = self.mode {
            // Enter inserts a newline in a comment, so the editor's own
            // Enter-commits rule doesn't apply: ZZ sends, ZQ discards.
            if k.code == KeyCode::Enter {
                self.input
                    .key(KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::NONE));
                return Action::None;
            }
            match self.input.key(k) {
                editor::Outcome::Continue => {}
                editor::Outcome::Commit => self.submit(),
                editor::Outcome::Cancel => {
                    self.mode = Mode::Normal;
                    self.status = Some("draft kept — c/R to continue".into());
                }
            }
            return Action::None;
        }

        if self.focus == Focus::Todo {
            // `t` toggles back even mid-list, but not while typing
            if !self.todo.editing()
                && !self.todo.picking()
                && k.code == KeyCode::Char('t')
                && !k.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.focus = Focus::Full;
                return Action::None;
            }
            return match self.todo.key(k, view_h) {
                ListAction::Quit => Action::Quit,
                // esc leaves the checklist for the ticket it belongs to
                ListAction::Back => {
                    self.focus = Focus::Full;
                    Action::None
                }
                ListAction::Reload => {
                    self.reload();
                    Action::None
                }
                // already looking at the ticket
                ListAction::Open(_) | ListAction::None => Action::None,
            };
        }

        self.status = None;
        let g = std::mem::take(&mut self.pending_g);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let half = (view_h / 2).max(1) as i32;

        match (k.code, ctrl) {
            (KeyCode::Char('q'), _) => return Action::Quit,
            (KeyCode::Char('c'), true) => return Action::Quit,
            // back to the checklist you came from; at the top level there is
            // nowhere to go, and popping the last view exits as it always did
            (KeyCode::Esc, _) => return Action::Pop,

            (KeyCode::Char('t'), false) => self.focus = Focus::Todo,

            (KeyCode::Char('j'), false) | (KeyCode::Down, _) => self.scroll_by(1, view_h),
            (KeyCode::Char('k'), false) | (KeyCode::Up, _) => self.scroll_by(-1, view_h),
            (KeyCode::Char('d'), _) => self.scroll_by(half, view_h),
            (KeyCode::Char('u'), _) => self.scroll_by(-half, view_h),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => {
                self.scroll_by(view_h as i32, view_h)
            }
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => {
                self.scroll_by(-(view_h as i32), view_h)
            }
            (KeyCode::Char('g'), false) => {
                if g {
                    self.scroll = 0;
                } else {
                    self.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) | (KeyCode::End, _) => self.scroll = self.max_scroll(view_h),
            (KeyCode::Home, _) => self.scroll = 0,

            (KeyCode::Char('J'), _) | (KeyCode::Char('n'), false) | (KeyCode::Char('l'), false) => {
                self.select_comment(1, view_h)
            }
            (KeyCode::Char('K'), _) | (KeyCode::Char('p'), false) | (KeyCode::Char('h'), false) => {
                self.select_comment(-1, view_h)
            }

            (KeyCode::Char('r'), false) => self.reload(),
            (KeyCode::Char('w'), false) => {
                jira::open_in_browser(&self.key);
                self.status = Some("opening in browser…".into());
            }
            (KeyCode::Char('c'), false) => self.mode = Mode::Compose { reply_to: None },
            (KeyCode::Char('R'), _) => match self.sel {
                Some(i) => self.mode = Mode::Compose { reply_to: Some(i) },
                None => {
                    self.status = Some("select a comment first (J/K), then R to reply".into())
                }
            },
            _ => {}
        }
        Action::None
    }
}
