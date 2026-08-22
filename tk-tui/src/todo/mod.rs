//! The aggregate checklist: one list over every open ticket's TODO section
//! plus the local file.

pub mod jira;
pub mod list;
pub mod local;
pub mod model;
pub mod sync;

use crate::rest::Config;
use anyhow::Result;
use list::{ItemList, ListAction};
use local::LocalFile;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct TodoView {
    pub list: ItemList,
    /// None until the first successful config read. A dead token costs you the
    /// ticket groups and a banner, not the pane.
    cfg: Option<Config>,
    jira_error: Option<String>,
}

impl TodoView {
    pub fn new() -> Result<Self> {
        let mut v = Self {
            list: ItemList::new(Some(LocalFile::load()?)),
            cfg: None,
            jira_error: None,
        };
        v.rebuild();
        Ok(v)
    }

    fn rebuild(&mut self) {
        let mut groups = vec![self
            .list
            .local
            .as_ref()
            .expect("the aggregate view always has one")
            .group()];
        self.jira_error = None;

        match self.fetch_jira() {
            Ok(issues) => {
                for issue in &issues {
                    groups.push(jira::group(issue));
                }
            }
            Err(e) => self.jira_error = Some(format!("{e:#}")),
        }
        self.list.groups = groups;
        self.list.clamp();
    }

    fn fetch_jira(&mut self) -> Result<Vec<jira::Issue>> {
        if self.cfg.is_none() {
            self.cfg = Some(crate::rest::config()?);
        }
        jira::search(self.cfg.as_ref().expect("just set"))
    }

    pub fn reload(&mut self) {
        match LocalFile::load() {
            Ok(f) => {
                self.list.local = Some(f);
                self.rebuild();
                self.list.status = Some(match &self.jira_error {
                    Some(e) => format!("local reloaded — jira: {e}"),
                    None => "refreshed".into(),
                });
            }
            Err(e) => self.list.status = Some(format!("reload failed: {e:#}")),
        }
    }

    /// Drain finished writes; a landed change is pulled back down from Jira so
    /// the list matches what's actually there.
    pub fn tick(&mut self) {
        if self.list.poll_sync() {
            let msg = self.list.status.clone();
            self.reload();
            self.list.status = msg;
        }
    }

    pub fn key(&mut self, k: KeyEvent, view_h: u16) -> ListAction {
        match self.list.key(k, view_h) {
            ListAction::Reload => {
                self.reload();
                ListAction::None
            }
            other => other,
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(f.area());
        self.list.draw(f, chunks[0]);

        let th = crate::theme::theme();
        let footer = match (&self.list.status, &self.jira_error) {
            _ if self.list.editing() || self.list.picking() => {
                Line::styled(self.list.hint(), Style::default().fg(th.dim))
            }
            (Some(s), _) => Line::styled(format!(" {s}"), Style::default().fg(th.status)),
            (None, Some(e)) => Line::styled(
                format!(" jira unavailable: {e}"),
                Style::default().fg(th.pending),
            ),
            (None, None) => Line::styled(self.list.hint(), Style::default().fg(th.dim)),
        };
        f.render_widget(Paragraph::new(footer), chunks[1]);
    }

    pub fn dump(&self) -> Vec<String> {
        self.list.dump()
    }
}
