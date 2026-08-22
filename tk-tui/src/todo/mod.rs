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

    /// Put the cursor on the item whose text is exactly `text`.
    ///
    /// Two things this gets right that the obvious version doesn't. It walks
    /// *cursor positions*, not items — an empty group is a position too, so a
    /// flat index over items drifts out of step as soon as one ticket has no
    /// checkboxes. And it matches the whole string: a substring match once
    /// caught a real item of the user's ("Another one" ends with "one") and
    /// indented it, which is exactly the kind of damage a live test must not
    /// be able to do.
    fn focus(v: &mut TodoView, text: &str) {
        for n in 0..v.list.target_count() {
            v.list.cursor = n;
            if let Some((gi, ii)) = v.list.at(n) {
                if v.list.groups[gi].items[ii].text == text {
                    return;
                }
            }
        }
        panic!("{text:?} is not in the list");
    }

    /// A prefix nothing of the user's will collide with.
    const TAG: &str = "tk-live-test";

    /// The full round trip: land on a ticket in the list, press enter, get the
    /// ticket pane, press esc, be back where you were.
    #[test]
    #[ignore = "talks to real Jira"]
    fn enter_opens_the_ticket_and_esc_comes_back() {
        use crate::view::{Action, View};

        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-1".into());
        let mut stack = vec![View::todo().expect("todo view")];

        // walk to the ticket, however far down it is
        if let View::Todo(v) = stack.last_mut().unwrap() {
            let gi = v
                .list
                .groups
                .iter()
                .position(|g| g.key.as_deref() == Some(key.as_str()))
                .unwrap_or_else(|| panic!("{key} not listed"));
            v.list.cursor = 0;
            for _ in 0..v.list.target_count() {
                if v.list.group_at_cursor() == Some(gi) {
                    break;
                }
                press(v, 'j');
            }
            assert_eq!(v.list.group_at_cursor(), Some(gi));
        }

        // enter
        let act = stack
            .last_mut()
            .unwrap()
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20)
            .expect("key");
        match act {
            Action::Push(v) => stack.push(*v),
            other => panic!("enter should push a view, got {other:?}"),
        }
        assert_eq!(stack.len(), 2);
        match stack.last().unwrap() {
            View::Ticket(a) => {
                assert_eq!(a.key, key);
                assert!(a.nested, "it must know it can go back");
                assert_eq!(
                    a.focus,
                    crate::app::Focus::Full,
                    "enter lands on the ticket itself, not its checklist"
                );
                println!("opened {} — {}", a.key, a.ticket.summary);
            }
            _ => panic!("expected a ticket view"),
        }

        // esc
        let act = stack
            .last_mut()
            .unwrap()
            .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 20)
            .expect("key");
        assert!(matches!(act, Action::Pop), "esc should pop, got {act:?}");
        stack.pop();
        assert_eq!(stack.len(), 1, "back at the checklist");
        assert!(matches!(stack.last().unwrap(), View::Todo(_)));
        println!("esc returned to the checklist");

        // q from the ticket closes the pane outright
        let mut t = View::ticket_from_list(&key).expect("ticket");
        let act = t
            .key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE), 20)
            .expect("key");
        assert!(matches!(act, Action::Quit), "q should quit, got {act:?}");
        println!("q quits");
    }

    /// A visual selection of several items, indented in one gesture, against
    /// the real Jira — the case where the per-item writes could tread on each
    /// other if they were applied in the wrong order.
    #[test]
    #[ignore = "writes to a real Jira ticket"]
    fn a_visual_selection_indents_together_in_the_real_ticket() {
        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-2".into());
        let cfg = crate::rest::config().expect("config");
        let (anchor, one, two) =
            (format!("{TAG} anchor"), format!("{TAG} one"), format!("{TAG} two"));

        // seed a parent and two siblings under it
        let doc = jira::fetch_description(&cfg, &key).expect("fetch");
        let (doc, a) = jira::add(doc.as_ref(), &anchor);
        let (doc, b) = jira::insert_after(&doc, &a, &one).expect("b");
        let (doc, c) = jira::insert_after(&doc, &b, &two).expect("c");
        jira::save_description(&cfg, &key, &doc).expect("seed");

        let mut v = TodoView::new().expect("view");

        // V on the first sibling, drag down to the second, then a single `>`
        focus(&mut v, &one);
        press(&mut v, 'V');
        press(&mut v, 'j');
        press(&mut v, '>');
        for _ in 0..150 {
            v.tick();
            if v.list.pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(v.list.pending(), 0, "writes never landed");
        println!("status: {:?}", v.list.status);

        let after = jira::fetch_description(&cfg, &key).expect("refetch").expect("doc");
        let items = jira::items(&key, &after);
        let depth = |text: &str| {
            items
                .iter()
                .find(|i| i.text == text)
                .unwrap_or_else(|| panic!("{text:?} gone"))
                .depth
        };
        assert_eq!(depth(&anchor), 0, "the anchor stays put");
        assert_eq!(depth(&one), 1, "both selected items nested");
        assert_eq!(depth(&two), 1);
        println!("anchor 0, one {}, two {}", depth(&one), depth(&two));

        let mut doc = after;
        for id in [&a, &b, &c] {
            doc = jira::remove(&doc, id).unwrap_or(doc);
        }
        jira::save_description(&cfg, &key, &doc).expect("cleanup");
        println!("cleaned up");
    }

    /// `>>` against the real Jira: indent an item, confirm the ticket's own
    /// description nests it, then put it back.
    #[test]
    #[ignore = "writes to a real Jira ticket"]
    fn double_angle_indents_an_item_in_the_real_ticket() {
        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-2".into());
        let cfg = crate::rest::config().expect("config");
        let (parent, child) = (format!("{TAG} parent"), format!("{TAG} child"));

        // give the ticket two items to work with
        let doc = jira::fetch_description(&cfg, &key).expect("fetch");
        let (doc, first) = jira::add(doc.as_ref(), &parent);
        let (doc, second) = jira::insert_after(&doc, &first, &child).expect("insert");
        jira::save_description(&cfg, &key, &doc).expect("seed");

        let mut v = TodoView::new().expect("view");
        let flat = v
            .list
            .groups
            .iter()
            .find(|g| g.key.as_deref() == Some(key.as_str()))
            .expect("group")
            .items
            .iter()
            .find(|i| i.text == child)
            .expect("the child item")
            .depth;
        assert_eq!(flat, 0, "starts flat");

        focus(&mut v, &child);
        press(&mut v, '>');
        press(&mut v, '>');
        for _ in 0..100 {
            v.tick();
            if v.list.pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(v.list.pending(), 0, "the write never landed");
        println!("status: {:?}", v.list.status);

        // ask Jira
        let after = jira::fetch_description(&cfg, &key)
            .expect("refetch")
            .expect("a doc");
        let items = jira::items(&key, &after);
        let moved = items
            .iter()
            .find(|i| i.text == child)
            .expect("child still there");
        assert_eq!(moved.depth, 1, "Jira has it nested; items: {items:?}");
        println!("indented in {key}: depth {}", moved.depth);

        // clean up both
        let mut doc = after;
        for id in [&first, &second] {
            doc = jira::remove(&doc, id).unwrap_or(doc);
        }
        jira::save_description(&cfg, &key, &doc).expect("cleanup");
        println!("cleaned up");
    }

    #[test]
    #[ignore = "writes to a real Jira ticket"]
    fn adds_to_an_empty_ticket_through_the_keys_you_actually_press() {
        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-2".into());
        let text = format!("{TAG} nav — safe to delete");
        let text = text.as_str();

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
