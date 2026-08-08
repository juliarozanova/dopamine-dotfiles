//! App state + the actions that mutate it. Terminal/event plumbing lives in
//! main.rs; rendering in ui.rs.

use crate::jira::{self, Ticket};
use crate::ui::{build, line_text, Segments};
use anyhow::Result;

pub enum Mode {
    Normal,
    /// Composing a comment; `reply_to` is an index into ticket.comments.
    Compose { reply_to: Option<usize> },
}

pub struct App {
    pub key: String,
    pub ticket: Ticket,
    pub segs: Segments,
    pub scroll: u16,
    pub sel: Option<usize>, // selected comment
    pub mode: Mode,
    pub input: String,
    pub status: Option<String>,
    pub pending_g: bool,
}

impl App {
    pub fn new(key: &str) -> Result<Self> {
        let ticket = jira::fetch(key)?;
        let segs = build(&ticket);
        Ok(Self {
            key: key.to_string(),
            ticket,
            segs,
            scroll: 0,
            sel: None,
            mode: Mode::Normal,
            input: String::new(),
            status: None,
            pending_g: false,
        })
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
        let text = self.input.trim().to_string();
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
                self.input.clear();
                self.mode = Mode::Normal;
                self.reload();
                self.status = Some("comment posted ✓".into());
            }
            // stay in compose so the draft isn't lost
            Err(e) => self.status = Some(format!("post failed: {e:#}")),
        }
    }
}
