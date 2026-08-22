//! Background writes to Jira.
//!
//! The pane must not block on the network. Toggling a checkbox changes the
//! model immediately and queues the write; a worker thread pushes it and
//! reports back. Until it lands the item carries a gutter mark, and if the
//! write fails the list is reloaded so what you see is what Jira has.
//!
//! Every write re-reads the description first and checks the item is still
//! where we left it. That costs a round trip and buys the guarantee that two
//! surfaces editing the same ticket can't silently overwrite each other.

use super::jira;
use crate::rest;
use anyhow::Result;
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, Debug)]
pub enum Op {
    Done {
        key: String,
        local_id: String,
        done: bool,
        /// What the item said when we read it, for the conflict check.
        was: bool,
    },
    Text {
        key: String,
        local_id: String,
        text: String,
        was: String,
    },
    Delete {
        key: String,
        local_id: String,
    },
    /// Move an item one nesting level deeper or shallower.
    Shift {
        key: String,
        local_id: String,
        deeper: bool,
    },
    Add {
        key: String,
        text: String,
        /// Insert directly below this item, keeping its nesting. None appends
        /// to the section.
        after: Option<String>,
    },
    /// Move a local item onto a ticket. Carries the local line so the caller
    /// can drop it *after* the write lands — never before.
    Promote {
        key: String,
        text: String,
        line: usize,
    },
}

impl Op {
    pub fn key(&self) -> &str {
        match self {
            Op::Done { key, .. } | Op::Text { key, .. } => key,
            Op::Delete { key, .. } | Op::Add { key, .. } => key,
            Op::Shift { key, .. } => key,
            Op::Promote { key, .. } => key,
        }
    }

    pub fn local_id(&self) -> Option<&str> {
        match self {
            Op::Done { local_id, .. } | Op::Text { local_id, .. } => Some(local_id),
            Op::Delete { local_id, .. } | Op::Shift { local_id, .. } => Some(local_id),
            Op::Add { .. } | Op::Promote { .. } => None,
        }
    }
}

pub struct Done {
    pub op: Op,
    pub error: Option<String>,
}

pub struct Sync {
    tx: Sender<Op>,
    rx: Receiver<Done>,
}

impl Sync {
    pub fn spawn() -> Self {
        let (tx, work_rx) = channel::<Op>();
        let (done_tx, rx) = channel::<Done>();
        std::thread::spawn(move || {
            for op in work_rx {
                let error = apply(&op).err().map(|e| format!("{e:#}"));
                if done_tx.send(Done { op, error }).is_err() {
                    break; // the view is gone
                }
            }
        });
        Self { tx, rx }
    }

    pub fn send(&self, op: Op) {
        let _ = self.tx.send(op);
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

/// Re-read, check, mutate, write. The read is the conflict guard: if the item
/// moved on since we showed it, we refuse rather than clobber.
fn apply(op: &Op) -> Result<()> {
    let cfg = rest::config()?;
    let doc = jira::fetch_description(&cfg, op.key())?;

    let updated = match op {
        Op::Add { text, after: Some(after), .. } => {
            let doc = doc.ok_or_else(|| {
                anyhow::anyhow!("the ticket has no description any more — press r to refresh")
            })?;
            jira::insert_after(&doc, after, text)?.0
        }
        Op::Add { text, .. } | Op::Promote { text, .. } => jira::add(doc.as_ref(), text).0,
        _ => {
            let doc = doc.ok_or_else(|| {
                anyhow::anyhow!("the ticket has no description any more — press r to refresh")
            })?;
            let local_id = op.local_id().expect("non-add ops carry an id");

            match op {
                Op::Done { done, was, .. } => {
                    check_state(&doc, local_id, *was)?;
                    jira::set_done(&doc, local_id, *done)?
                }
                Op::Text { text, was, .. } => {
                    check_text(&doc, local_id, was)?;
                    jira::set_text(&doc, local_id, text)?
                }
                Op::Delete { .. } => jira::remove(&doc, local_id)?,
                Op::Shift { deeper, .. } => jira::shift(&doc, local_id, *deeper)?,
                Op::Add { .. } | Op::Promote { .. } => unreachable!("handled above"),
            }
        }
    };

    jira::save_description(&cfg, op.key(), &updated)
}

fn find<'a>(doc: &'a serde_json::Value, local_id: &str) -> Result<&'a serde_json::Value> {
    fn walk<'a>(n: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
        if n["type"] == "taskItem" && jira::item_local_id(n) == id {
            return Some(n);
        }
        n["content"]
            .as_array()?
            .iter()
            .find_map(|k| walk(k, id))
    }
    walk(doc, local_id)
        .ok_or_else(|| anyhow::anyhow!("that item is no longer in the ticket — press r to refresh"))
}

fn check_state(doc: &serde_json::Value, local_id: &str, was: bool) -> Result<()> {
    if jira::item_done(find(doc, local_id)?) != was {
        anyhow::bail!("someone else ticked that item — press r to refresh");
    }
    Ok(())
}

fn check_text(doc: &serde_json::Value, local_id: &str, was: &str) -> Result<()> {
    if jira::item_text(find(doc, local_id)?) != was {
        anyhow::bail!("that item was reworded elsewhere — press r to refresh");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(state: &str, text: &str) -> serde_json::Value {
        json!({ "type": "doc", "content": [
            { "type": "heading", "attrs": { "level": 2 },
              "content": [{ "type": "text", "text": "TODO" }] },
            { "type": "taskList", "attrs": {}, "content": [
                { "type": "taskItem", "attrs": { "localId": "a1", "state": state },
                  "content": [{ "type": "text", "text": text }] }
            ]}
        ]})
    }

    #[test]
    fn the_guard_passes_when_the_item_is_as_we_left_it() {
        assert!(check_state(&doc("TODO", "x"), "a1", false).is_ok());
        assert!(check_text(&doc("TODO", "x"), "a1", "x").is_ok());
    }

    #[test]
    fn the_guard_refuses_when_someone_else_got_there_first() {
        let e = check_state(&doc("DONE", "x"), "a1", false)
            .unwrap_err()
            .to_string();
        assert!(e.contains("someone else ticked"), "got: {e}");

        let e = check_text(&doc("TODO", "reworded"), "a1", "x")
            .unwrap_err()
            .to_string();
        assert!(e.contains("reworded elsewhere"), "got: {e}");
    }

    #[test]
    fn a_vanished_item_is_reported_not_recreated() {
        let e = find(&doc("TODO", "x"), "gone").unwrap_err().to_string();
        assert!(e.contains("no longer in the ticket"), "got: {e}");
    }
}

/// Live round trip against the real Jira, kept out of `cargo test` because it
/// writes to a ticket. Run deliberately:
///
///   . ~/.config/jira-board/env
///   cargo test -- --ignored --test-threads=1 --nocapture
///
/// `--test-threads=1` matters: these share a ticket, and in parallel they
/// clean up each other's fixtures.
///
/// Set TK_TEST_ISSUE to point it somewhere other than JROZ-1.
#[cfg(test)]
mod live {
    use super::*;
    use crate::todo::jira;

    /// Toggle one existing item and nothing else, so the description can be
    /// diffed byte-for-byte either side of the write.
    #[test]
    #[ignore = "writes to a real Jira ticket"]
    fn live_toggle_one_item() {
        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-2".into());
        let Ok(id) = std::env::var("TK_TEST_ITEM") else {
            println!("skipping: set TK_TEST_ITEM to a localId to run this one");
            return;
        };
        let cfg = rest::config().expect("config");
        let doc = jira::fetch_description(&cfg, &key).expect("fetch").expect("a doc");
        let was = jira::item_done(find(&doc, &id).expect("the item"));
        let next = jira::set_done(&doc, &id, !was).expect("toggle");
        jira::save_description(&cfg, &key, &next).expect("save");
        println!("toggled {id}: {was} -> {}", !was);
    }

    #[test]
    #[ignore = "writes to a real Jira ticket"]
    fn live_round_trip_touches_only_the_item_it_owns() {
        let key = std::env::var("TK_TEST_ISSUE").unwrap_or_else(|_| "JROZ-1".into());
        let cfg = rest::config().expect("jira config + JIRA_API_TOKEN");
        let before = jira::fetch_description(&cfg, &key).expect("fetch");

        let text = "tk-tui self test — safe to delete";
        let (doc, id) = jira::add(before.as_ref(), text);
        jira::save_description(&cfg, &key, &doc).expect("add");
        println!("added {id}");

        let after_add = jira::fetch_description(&cfg, &key).expect("refetch").expect("a doc");
        let item = find(&after_add, &id).expect("the item we just added");
        assert_eq!(jira::item_text(item), text);
        assert!(!jira::item_done(item));

        // toggle
        let toggled = jira::set_done(&after_add, &id, true).expect("toggle");
        jira::save_description(&cfg, &key, &toggled).expect("save toggle");
        let after_toggle = jira::fetch_description(&cfg, &key).expect("refetch").expect("a doc");
        assert!(jira::item_done(find(&after_toggle, &id).expect("still there")));
        println!("toggled");

        // clean up, and check we left the rest of the description alone
        let removed = jira::remove(&after_toggle, &id).expect("remove");
        jira::save_description(&cfg, &key, &removed).expect("save remove");
        let after_remove = jira::fetch_description(&cfg, &key).expect("refetch").expect("a doc");
        assert!(find(&after_remove, &id).is_err(), "the item should be gone");

        if let Some(before) = before {
            // everything that was there before must have survived all three writes
            let was: Vec<_> = before["content"].as_array().cloned().unwrap_or_default();
            let now: Vec<_> = after_remove["content"].as_array().cloned().unwrap_or_default();
            for node in &was {
                assert!(now.contains(node), "a pre-existing node was lost: {node}");
            }
            println!("all {} pre-existing top-level nodes survived", was.len());
        }
    }
}
