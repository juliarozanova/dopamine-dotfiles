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

/// End-to-end through the real keypresses and the real Jira: navigate to a
/// ticket that has no checkboxes, press `o`, type, and confirm the item lands
/// in the ticket's description. Ignored by default because it writes.
///
///   . ~/.config/jira-board/env
///   cargo test -- --ignored --nocapture adds_to_an_empty_ticket
#[cfg(test)]
mod live {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn press(v: &mut TodoView, c: char) {
        v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 20);
    }

    #[test]
    #[ignore = "writes to a real Jira ticket"]
    fn adds_to_an_empty_ticket_through_the_keys_you_actually_press() {
        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-2".into());
        let text = "tk-tui nav test — safe to delete";

        let mut v = TodoView::new().expect("todo view");
        let gi = v
            .list
            .groups
            .iter()
            .position(|g| g.key.as_deref() == Some(key.as_str()))
            .unwrap_or_else(|| panic!("{key} not in the list"));
        println!("group {gi} is {key}, {} items", v.list.groups[gi].items.len());

        // walk there with j, exactly as you would
        v.list.cursor = 0;
        for _ in 0..v.list.target_count() {
            if v.list.group_at_cursor() == Some(gi) {
                break;
            }
            press(&mut v, 'j');
        }
        assert_eq!(v.list.group_at_cursor(), Some(gi), "j must reach {key}");

        press(&mut v, 'o');
        assert!(v.list.editing(), "o must open an editor");
        for c in text.chars() {
            press(&mut v, c);
        }
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20);

        // let the sync worker land it
        for _ in 0..100 {
            v.tick();
            if v.list.pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(v.list.pending(), 0, "the write never completed");
        println!("status: {:?}", v.list.status);

        // ask Jira directly
        let cfg = crate::rest::config().expect("config");
        let doc = jira::fetch_description(&cfg, &key)
            .expect("refetch")
            .expect("a description");
        let items = jira::items(&key, &doc);
        let found = items.iter().find(|i| i.text == text);
        assert!(found.is_some(), "not in the ticket; got {items:?}");
        println!("landed in {key}: {:?}", found.map(|i| &i.text));

        // and the view shows it under that ticket, not under "no ticket"
        let g = v
            .list
            .groups
            .iter()
            .find(|g| g.key.as_deref() == Some(key.as_str()))
            .expect("group");
        assert!(g.items.iter().any(|i| i.text == text), "not shown under {key}");

        // clean up
        let id = match &found.unwrap().origin {
            model::Origin::Jira { local_id, .. } => local_id.clone(),
            _ => panic!("expected a jira origin"),
        };
        let pruned = jira::remove(&doc, &id).expect("remove");
        jira::save_description(&cfg, &key, &pruned).expect("save");
        println!("cleaned up");
    }
}
