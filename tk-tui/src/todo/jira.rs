//! The Jira half of the checklist: find the TODO section in a description,
//! and edit it without disturbing anything else.
//!
//! The rule this module exists to enforce: **we never re-serialise a
//! description.** The fetched ADF document is kept as an opaque `Value` and
//! mutated in place, so panels, tables, media, mentions and smart links — every
//! node type this code has never heard of — come back byte-identical. That is
//! what makes writing to a field you only partly own safe, and it's cheaper
//! than understanding ADF would be.

use super::model::{Origin, TodoGroup, TodoItem};
use crate::adf;
use crate::rest::{self, Config};
use crate::ui::line_text;
use anyhow::{Context, Result};
use serde_json::{json, Value};

/// The heading that scopes the list. Checkboxes elsewhere in a description —
/// acceptance criteria, a stray checklist in the notes — are deliberately
/// ignored, so the global list stays yours.
pub const SECTION: &str = "TODO";

pub struct Issue {
    pub key: String,
    pub summary: String,
    pub description: Option<Value>,
}

/// Every open assigned issue, with descriptions, in one request.
pub fn search(cfg: &Config) -> Result<Vec<Issue>> {
    let jql = rest::todo_jql(cfg);
    let path = format!(
        "/rest/api/3/search/jql?jql={}&fields={}&maxResults=100",
        rest::encode(&jql),
        rest::encode("summary,description"),
    );
    let v = rest::get(cfg, &path)?;
    let issues = v["issues"].as_array().cloned().unwrap_or_default();
    Ok(issues
        .iter()
        .map(|i| Issue {
            key: i["key"].as_str().unwrap_or_default().to_string(),
            summary: i["fields"]["summary"].as_str().unwrap_or_default().to_string(),
            description: match &i["fields"]["description"] {
                Value::Null => None,
                d => Some(d.clone()),
            },
        })
        .collect())
}

pub fn fetch_description(cfg: &Config, key: &str) -> Result<Option<Value>> {
    let v = rest::get(cfg, &format!("/rest/api/3/issue/{key}?fields=description"))?;
    Ok(match &v["fields"]["description"] {
        Value::Null => None,
        d => Some(d.clone()),
    })
}

pub fn save_description(cfg: &Config, key: &str, doc: &Value) -> Result<()> {
    rest::put(
        cfg,
        &format!("/rest/api/3/issue/{key}"),
        &json!({ "fields": { "description": doc } }),
    )
}

fn top(doc: &Value) -> &[Value] {
    doc["content"].as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

/// Flatten a node's text, for comparing a heading against SECTION.
fn text_of(node: &Value) -> String {
    adf::to_lines(node)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Index range of the TODO section's body within `doc.content`: everything
/// after the TODO heading, up to the next heading at the same level or higher.
pub fn section_range(doc: &Value) -> Option<std::ops::Range<usize>> {
    let nodes = top(doc);
    let (start, level) = nodes.iter().enumerate().find_map(|(i, n)| {
        (n["type"] == "heading" && text_of(n).eq_ignore_ascii_case(SECTION))
            .then(|| (i, n["attrs"]["level"].as_u64().unwrap_or(1)))
    })?;
    let end = nodes
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, n)| {
            n["type"] == "heading" && n["attrs"]["level"].as_u64().unwrap_or(1) <= level
        })
        .map(|(i, _)| i)
        .unwrap_or(nodes.len());
    Some(start + 1..end)
}

/// Walk `taskItem`s in document order, calling `f` with each.
fn walk_task_items<'a>(node: &'a Value, f: &mut impl FnMut(&'a Value)) {
    if node["type"] == "taskItem" {
        f(node);
    }
    if let Some(kids) = node["content"].as_array() {
        for k in kids {
            walk_task_items(k, f);
        }
    }
}

/// Note `get_mut`, not `node["content"]`: serde_json's IndexMut *inserts* a
/// null for a missing key, so indexing our way down a document would quietly
/// stamp `"content": null` onto every text node we walked past. The whole
/// point of this module is that untouched nodes come back unchanged.
fn walk_task_items_mut(node: &mut Value, f: &mut impl FnMut(&mut Value) -> bool) -> bool {
    if node.get("type").map(|t| t == "taskItem").unwrap_or(false) && f(node) {
        return true;
    }
    if let Some(kids) = node.get_mut("content").and_then(Value::as_array_mut) {
        for k in kids {
            if walk_task_items_mut(k, f) {
                return true;
            }
        }
    }
    false
}

pub fn item_text(node: &Value) -> String {
    adf::inline_text(node)
}

pub fn item_done(node: &Value) -> bool {
    node["attrs"]["state"].as_str() == Some("DONE")
}

pub fn item_local_id(node: &Value) -> String {
    node["attrs"]["localId"].as_str().unwrap_or_default().to_string()
}

/// The items in a description's TODO section, in document order.
pub fn items(key: &str, doc: &Value) -> Vec<TodoItem> {
    let Some(range) = section_range(doc) else {
        return Vec::new();
    };
    let nodes = top(doc);
    let mut out = Vec::new();
    for n in &nodes[range] {
        walk_task_items(n, &mut |t| {
            out.push(TodoItem {
                text: item_text(t),
                done: item_done(t),
                origin: Origin::Jira {
                    key: key.to_string(),
                    local_id: item_local_id(t),
                },
                dirty: false,
            });
        });
    }
    out
}

pub fn group(issue: &Issue) -> TodoGroup {
    TodoGroup {
        title: issue.summary.clone(),
        key: Some(issue.key.clone()),
        items: issue
            .description
            .as_ref()
            .map(|d| items(&issue.key, d))
            .unwrap_or_default(),
    }
}

// ------------------------------------------------------------- mutations ---
//
// Each takes the fetched doc and returns a modified clone. Nothing outside the
// targeted taskItem is touched.

/// A localId unique within this process and unlikely to collide with Jira's.
pub fn new_local_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("tk-{nanos:x}-{:x}", N.fetch_add(1, Ordering::Relaxed))
}

fn with_item(doc: &Value, local_id: &str, f: impl FnOnce(&mut Value)) -> Result<Value> {
    let mut out = doc.clone();
    let mut f = Some(f);
    let hit = out
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .map(|nodes| {
            nodes.iter_mut().any(|n| {
                walk_task_items_mut(n, &mut |t| {
                    if item_local_id(t) == local_id {
                        if let Some(f) = f.take() {
                            f(t);
                        }
                        return true;
                    }
                    false
                })
            })
        })
        .unwrap_or(false);
    if !hit {
        anyhow::bail!("that item is no longer in the ticket — press r to refresh");
    }
    Ok(out)
}

pub fn set_done(doc: &Value, local_id: &str, done: bool) -> Result<Value> {
    with_item(doc, local_id, |t| {
        t["attrs"]["state"] = json!(if done { "DONE" } else { "TODO" });
    })
}

/// Replace an item's text.
///
/// This is the one lossy operation: an item's inline content can carry marks
/// (bold, links, mentions) and we can only write back a plain run. So callers
/// must only reach here when the text actually changed — an untouched item is
/// never rewritten, and therefore never flattened.
pub fn set_text(doc: &Value, local_id: &str, text: &str) -> Result<Value> {
    with_item(doc, local_id, |t| {
        t["content"] = json!([{ "type": "text", "text": text }]);
    })
}

pub fn remove(doc: &Value, local_id: &str) -> Result<Value> {
    let mut out = doc.clone();
    fn prune(node: &mut Value, local_id: &str) -> bool {
        let Some(kids) = node.get_mut("content").and_then(Value::as_array_mut) else {
            return false;
        };
        let before = kids.len();
        kids.retain(|k| !(k["type"] == "taskItem" && item_local_id(k) == local_id));
        if kids.len() != before {
            return true;
        }
        kids.iter_mut().any(|k| prune(k, local_id))
    }
    if !prune(&mut out, local_id) {
        anyhow::bail!("that item is no longer in the ticket — press r to refresh");
    }
    Ok(out)
}

/// Append an item to the TODO section, creating the heading and task list if
/// the ticket doesn't have them yet. Returns the new doc and the item's id.
pub fn add(doc: Option<&Value>, text: &str) -> (Value, String) {
    let local_id = new_local_id();
    let item = json!({
        "type": "taskItem",
        "attrs": { "localId": local_id, "state": "TODO" },
        "content": [{ "type": "text", "text": text }],
    });

    let mut out = match doc {
        Some(d) if d["type"] == "doc" => d.clone(),
        // No description at all, or something we don't recognise as a doc.
        _ => json!({ "type": "doc", "version": 1, "content": [] }),
    };
    if !out["content"].is_array() {
        out["content"] = json!([]);
    }

    // An existing task list inside the section takes the item…
    if let Some(range) = section_range(&out) {
        let end = range.end;
        let nodes = out["content"].as_array_mut().expect("set above");
        if let Some(list) = nodes[range]
            .iter_mut()
            .rev()
            .find(|n| n["type"] == "taskList")
        {
            match list.get_mut("content").and_then(Value::as_array_mut) {
                Some(kids) => kids.push(item),
                None => list["content"] = json!([item]),
            }
            return (out, local_id);
        }
        // …else the section exists but holds no list yet: put one at its end,
        // before whatever heading closes the section.
        let list = json!({ "type": "taskList", "attrs": { "localId": new_local_id() },
                           "content": [item] });
        nodes.insert(end, list);
        return (out, local_id);
    }

    // No section: append the heading and a fresh list at the end, leaving
    // whatever the description already said untouched above it.
    let nodes = out["content"].as_array_mut().expect("set above");
    nodes.push(json!({
        "type": "heading",
        "attrs": { "level": 2 },
        "content": [{ "type": "text", "text": SECTION }],
    }));
    nodes.push(json!({
        "type": "taskList",
        "attrs": { "localId": new_local_id() },
        "content": [item],
    }));
    (out, local_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, state: &str, text: &str) -> Value {
        json!({
            "type": "taskItem",
            "attrs": { "localId": id, "state": state },
            "content": [{ "type": "text", "text": text }],
        })
    }

    fn heading(level: u64, text: &str) -> Value {
        json!({ "type": "heading", "attrs": { "level": level },
                "content": [{ "type": "text", "text": text }] })
    }

    /// A description shaped like a real one: rich nodes this code has never
    /// heard of, checkboxes both inside and outside the TODO section.
    fn doc() -> Value {
        json!({ "type": "doc", "version": 1, "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "context" }] },
            heading(2, "Acceptance criteria"),
            { "type": "taskList", "attrs": { "localId": "ac" }, "content": [
                task("ac1", "TODO", "reviewed by two people")
            ]},
            heading(2, "TODO"),
            { "type": "taskList", "attrs": { "localId": "tl" }, "content": [
                task("a1", "TODO", "wire up retry"),
                task("b2", "DONE", "fix cache key")
            ]},
            heading(2, "Notes"),
            { "type": "table", "attrs": { "layout": "default" }, "content": [
                { "type": "tableRow", "content": [
                    { "type": "tableCell", "attrs": {}, "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "cell" }] }
                    ]}
                ]}
            ]},
            { "type": "mediaSingle", "attrs": { "layout": "center" }, "content": [
                { "type": "media", "attrs": { "id": "abc", "type": "file",
                                              "collection": "c" } }
            ]},
            { "type": "panel", "attrs": { "panelType": "info" }, "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "heads up" }] }
            ]},
            { "type": "taskList", "attrs": { "localId": "nl" }, "content": [
                task("n1", "TODO", "a checkbox in the notes")
            ]}
        ]})
    }

    #[test]
    fn the_section_stops_at_the_next_heading_of_the_same_level() {
        let d = doc();
        let r = section_range(&d).expect("a TODO heading");
        let nodes = top(&d);
        assert_eq!(nodes[r.clone()].len(), 1, "just the task list");
        assert_eq!(nodes[r.start]["type"], "taskList");
        assert_eq!(nodes[r.end]["type"], "heading");
    }

    #[test]
    fn only_checkboxes_under_the_todo_heading_are_collected() {
        let got: Vec<_> = items("JROZ-1", &doc())
            .into_iter()
            .map(|i| (i.text, i.done))
            .collect();
        assert_eq!(
            got,
            vec![
                ("wire up retry".to_string(), false),
                ("fix cache key".to_string(), true)
            ],
            "acceptance criteria and the notes checkbox must not appear"
        );
    }

    #[test]
    fn a_description_with_no_todo_heading_yields_nothing() {
        let d = json!({ "type": "doc", "version": 1, "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "hi" }] }
        ]});
        assert_eq!(section_range(&d), None);
        assert!(items("JROZ-1", &d).is_empty());
    }

    #[test]
    fn a_deeper_heading_does_not_end_the_section() {
        let d = json!({ "type": "doc", "content": [
            heading(2, "TODO"),
            heading(3, "later"),
            { "type": "taskList", "attrs": {}, "content": [task("x", "TODO", "still mine")] },
            heading(2, "Notes"),
        ]});
        assert_eq!(items("K", &d).len(), 1);
    }

    /// THE test. Everything this code has never heard of — the table, the
    /// media node, the panel, the checkbox outside the section — must come back
    /// byte-identical. If this fails, "surgical" was a hope, not a property.
    #[test]
    fn toggling_changes_exactly_one_field_and_nothing_else() {
        let before = doc();
        let after = set_done(&before, "b2", false).unwrap();

        assert_ne!(before, after, "the toggle must actually do something");
        assert_eq!(after["content"][4]["content"][1]["attrs"]["state"], "TODO");

        // reverting that one field must reproduce the original exactly
        let reverted = set_done(&after, "b2", true).unwrap();
        assert_eq!(reverted, before, "everything outside the item must survive");
    }

    #[test]
    fn retexting_changes_exactly_one_item() {
        let before = doc();
        let after = set_text(&before, "a1", "wire up retry with backoff").unwrap();
        assert_eq!(item_text(&after["content"][4]["content"][0]), "wire up retry with backoff");

        let mut stripped = after.clone();
        stripped["content"][4]["content"][0]["content"] =
            before["content"][4]["content"][0]["content"].clone();
        assert_eq!(stripped, before);
    }

    #[test]
    fn editing_an_item_that_vanished_is_an_error_not_a_silent_no_op() {
        let e = set_done(&doc(), "gone", true).unwrap_err().to_string();
        assert!(e.contains("no longer in the ticket"), "got: {e}");
        assert!(remove(&doc(), "gone").is_err());
    }

    #[test]
    fn removing_takes_the_item_and_leaves_its_siblings() {
        let after = remove(&doc(), "a1").unwrap();
        let list = &after["content"][4]["content"];
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(item_local_id(&list[0]), "b2");
        // the checkbox outside the section is untouched
        assert_eq!(after["content"][8], doc()["content"][8]);
    }

    #[test]
    fn adding_appends_to_the_existing_list_in_the_section() {
        let before = doc();
        let (after, id) = add(Some(&before), "a third thing");
        let list = after["content"][4]["content"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(item_local_id(&list[2]), id);
        assert_eq!(item_text(&list[2]), "a third thing");
        assert!(!item_done(&list[2]));
        // nothing else moved
        assert_eq!(after["content"][6], before["content"][6]);
    }

    #[test]
    fn adding_to_a_ticket_with_no_section_appends_a_heading_and_list() {
        let before = json!({ "type": "doc", "version": 1, "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "just prose" }] }
        ]});
        let (after, id) = add(Some(&before), "first thing");
        let nodes = after["content"].as_array().unwrap();
        assert_eq!(nodes[0], before["content"][0], "the prose survives");
        assert_eq!(nodes[1]["type"], "heading");
        assert_eq!(text_of(&nodes[1]), SECTION);
        assert_eq!(nodes[2]["type"], "taskList");
        assert_eq!(items("K", &after).len(), 1);
        assert_eq!(item_local_id(&nodes[2]["content"][0]), id);
    }

    #[test]
    fn adding_to_a_ticket_with_no_description_builds_a_whole_doc() {
        let (after, _) = add(None, "first thing");
        assert_eq!(after["type"], "doc");
        assert_eq!(after["version"], 1);
        assert_eq!(items("K", &after).len(), 1);
    }

    #[test]
    fn adding_under_an_empty_todo_heading_creates_the_list() {
        let before = json!({ "type": "doc", "version": 1, "content": [heading(2, "TODO")] });
        let (after, _) = add(Some(&before), "first thing");
        assert_eq!(items("K", &after).len(), 1);
    }

    #[test]
    fn a_lowercase_heading_still_scopes_the_section() {
        let d = json!({ "type": "doc", "content": [
            heading(3, "todo"),
            { "type": "taskList", "attrs": {}, "content": [task("x", "TODO", "yes")] },
        ]});
        assert_eq!(items("K", &d).len(), 1);
    }

    /// Marks inside an item survive as long as its text is untouched — which
    /// is why callers must compare before calling set_text.
    #[test]
    fn an_item_with_marks_keeps_them_through_a_toggle() {
        let before = json!({ "type": "doc", "content": [
            heading(2, "TODO"),
            { "type": "taskList", "attrs": {}, "content": [
                { "type": "taskItem", "attrs": { "localId": "m1", "state": "TODO" },
                  "content": [
                    { "type": "text", "text": "see ", },
                    { "type": "text", "text": "the docs",
                      "marks": [{ "type": "link", "attrs": { "href": "https://x" } }] }
                  ]}
            ]}
        ]});
        let after = set_done(&before, "m1", true).unwrap();
        assert_eq!(
            after["content"][1]["content"][0]["content"],
            before["content"][1]["content"][0]["content"]
        );
        assert_eq!(item_text(&after["content"][1]["content"][0]), "see the docs");
    }
}
