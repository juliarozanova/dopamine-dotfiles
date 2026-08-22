//! Minimal ADF (Atlassian Document Format) → ratatui Lines.
//! Covers the nodes that actually appear in tickets: paragraphs, headings,
//! bullet/ordered lists, task lists (Jira's checkboxes), code blocks,
//! blockquotes, rules, hard breaks, and the strong/em/code/link marks.
//! Unknown nodes fall back to their children.

use crate::theme::theme;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;

pub fn to_lines(adf: &Value) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    blocks(adf, "", "", &mut out);
    // trim trailing blank lines
    while matches!(out.last(), Some(l) if l.width() == 0) {
        out.pop();
    }
    out
}

fn kids(v: &Value) -> &[Value] {
    v["content"].as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

/// Render block-level nodes. `first`/`rest` are prefixes for the first and
/// subsequent lines this node emits (used for list bullets / quote bars).
fn blocks(v: &Value, first: &str, rest: &str, out: &mut Vec<Line<'static>>) {
    match v["type"].as_str().unwrap_or("") {
        "doc" => {
            for (i, c) in kids(v).iter().enumerate() {
                if i > 0 {
                    out.push(Line::default());
                }
                blocks(c, first, rest, out);
            }
        }
        "paragraph" => {
            let mut pfx = first;
            for spans in inline_lines(v) {
                let mut l = vec![Span::raw(pfx.to_string())];
                l.extend(spans);
                out.push(Line::from(l));
                pfx = rest;
            }
            if kids(v).is_empty() {
                out.push(Line::raw(first.to_string()));
            }
        }
        "heading" => {
            for spans in inline_lines(v) {
                let mut l = vec![Span::raw(first.to_string())];
                l.extend(spans.into_iter().map(|s| s.add_modifier(Modifier::BOLD)));
                out.push(Line::from(l));
            }
        }
        "bulletList" | "orderedList" => {
            let ordered = v["type"] == "orderedList";
            for (i, item) in kids(v).iter().enumerate() {
                let bullet = if ordered {
                    format!("{}{}. ", first, i + 1)
                } else {
                    format!("{}• ", first)
                };
                let cont = format!("{}{}", rest, " ".repeat(bullet.len() - first.len()));
                blocks(item, &bullet, &cont, out);
            }
        }
        // Jira's action items. Rendered here so a checkbox in the *description*
        // still reads as a checkbox in the ticket pane; the editable list is
        // built from the same nodes in todo::jira.
        "taskList" => {
            for item in kids(v) {
                blocks(item, first, rest, out);
            }
        }
        "taskItem" => {
            let done = v["attrs"]["state"].as_str() == Some("DONE");
            let (glyph, style) = if done {
                ("☑ ", Style::default().fg(theme().done))
            } else {
                ("☐ ", Style::default().fg(theme().checkbox))
            };
            let body: Vec<Span<'static>> = if done {
                inline_lines(v)
                    .into_iter()
                    .flatten()
                    .map(|s| s.add_modifier(Modifier::CROSSED_OUT))
                    .collect()
            } else {
                inline_lines(v).into_iter().flatten().collect()
            };
            let mut line = vec![
                Span::raw(first.to_string()),
                Span::styled(glyph.to_string(), style),
            ];
            line.extend(body);
            out.push(Line::from(line));
        }
        "listItem" => {
            let mut f = first.to_string();
            for c in kids(v) {
                blocks(c, &f, rest, out);
                f = rest.to_string(); // only the first block gets the bullet
            }
        }
        "codeBlock" => {
            let style = Style::default().fg(theme().code);
            for c in kids(v) {
                for part in c["text"].as_str().unwrap_or("").split('\n') {
                    out.push(Line::from(vec![
                        Span::raw(format!("{first}  ")),
                        Span::styled(part.to_string(), style),
                    ]));
                }
            }
        }
        "blockquote" => {
            let f = format!("{first}> ");
            let r = format!("{rest}> ");
            for c in kids(v) {
                blocks(c, &f, &r, out);
            }
        }
        "rule" => out.push(Line::styled(
            "────────────".to_string(),
            Style::default().fg(theme().rule),
        )),
        // mediaGroup, panel, table, unknown … : just descend
        _ => {
            for c in kids(v) {
                blocks(c, first, rest, out);
            }
        }
    }
}

/// Render a node's inline content, splitting on hardBreak.
fn inline_lines(v: &Value) -> Vec<Vec<Span<'static>>> {
    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for c in kids(v) {
        match c["type"].as_str().unwrap_or("") {
            "hardBreak" => lines.push(Vec::new()),
            "text" => {
                let text = c["text"].as_str().unwrap_or("").to_string();
                lines.last_mut().unwrap().push(styled(text, &c["marks"]));
            }
            "mention" => lines.last_mut().unwrap().push(Span::styled(
                c["attrs"]["text"].as_str().unwrap_or("@?").to_string(),
                Style::default().fg(theme().link),
            )),
            "emoji" => lines
                .last_mut()
                .unwrap()
                .push(Span::raw(c["attrs"]["text"].as_str().unwrap_or("").to_string())),
            "inlineCard" => lines.last_mut().unwrap().push(Span::styled(
                c["attrs"]["url"].as_str().unwrap_or("[card]").to_string(),
                Style::default().add_modifier(Modifier::UNDERLINED),
            )),
            _ => {
                for sub in inline_lines(c) {
                    lines.last_mut().unwrap().extend(sub);
                }
            }
        }
    }
    if lines.len() == 1 && lines[0].is_empty() {
        return Vec::new();
    }
    lines
}

/// A node's inline content as plain text, hard breaks flattened to spaces.
/// Used for checklist item text, where a single line is all there is room for.
pub fn inline_text(v: &Value) -> String {
    inline_lines(v)
        .iter()
        .map(|spans| {
            spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn styled(text: String, marks: &Value) -> Span<'static> {
    let mut style = Style::default();
    if let Some(ms) = marks.as_array() {
        for m in ms {
            match m["type"].as_str().unwrap_or("") {
                "strong" => style = style.add_modifier(Modifier::BOLD),
                "em" => style = style.add_modifier(Modifier::ITALIC),
                "code" => style = style.fg(theme().code_inline),
                "link" => style = style.add_modifier(Modifier::UNDERLINED).fg(theme().link),
                "strike" => style = style.add_modifier(Modifier::CROSSED_OUT),
                _ => {}
            }
        }
    }
    Span::styled(text, style)
}
