//! Rendering for the checklist. Shared by the aggregate todo view and the
//! ticket pane's TODO mode, so the two can never drift apart.

use crate::theme::theme;
use crate::todo::model::TodoGroup;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Something the cursor can sit on.
///
/// An empty group is a position in its own right, not a gap to skip: it's the
/// only way to reach a ticket that has no checkboxes yet, which is exactly
/// where you want to press `o`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sel {
    Item(usize, usize),
    EmptyGroup(usize),
}

impl Sel {
    pub fn group(self) -> usize {
        match self {
            Sel::Item(gi, _) | Sel::EmptyGroup(gi) => gi,
        }
    }
}

/// One rendered row. `sel` is what the cursor lands on here, or None for
/// headings and spacers.
pub struct Row {
    pub line: Line<'static>,
    pub sel: Option<Sel>,
    /// The row holding the live edit buffer, if any.
    pub editing: bool,
}

/// The in-progress edit, so the buffer is rendered *in place* rather than
/// patched over a row afterwards. A new item has no index to look up yet —
/// getting that wrong is why it used to render nowhere at all on an empty
/// list, and over the next group's row otherwise.
#[derive(Clone, Copy)]
pub enum Editing<'a> {
    None,
    /// Replacing the text of an existing item.
    Existing {
        gi: usize,
        ii: usize,
        text: &'a str,
        cursor: usize,
        insert: bool,
    },
    /// A new item, appended to group `gi`.
    New {
        gi: usize,
        text: &'a str,
        cursor: usize,
        insert: bool,
        depth: usize,
    },
}

/// Which items survive `hide_done`, per group, in display order.
///
/// Resolved once and used by both `rows` and `selectables`, so the cursor can
/// never address something the view isn't showing.
///
/// A **done parent with unfinished children stays**: hiding it would strand
/// its sub-items under nothing, indented into thin air. Ticking the parent is
/// not the same claim as finishing the work under it.
pub fn shown(g: &TodoGroup, hide_done: bool) -> Vec<usize> {
    if !hide_done {
        return (0..g.items.len()).collect();
    }
    (0..g.items.len())
        .filter(|&i| !g.items[i].done || has_open_child(g, i))
        .collect()
}

/// Any unfinished item nested under `i` — the run of following items deeper
/// than it, which is how both backends express nesting.
fn has_open_child(g: &TodoGroup, i: usize) -> bool {
    let depth = g.items[i].depth;
    g.items[i + 1..]
        .iter()
        .take_while(|it| it.depth > depth)
        .any(|it| !it.done)
}

fn dim() -> Style {
    Style::default().fg(theme().dim)
}

/// The gutter mark: a dot while a write is in flight, else a space so the
/// text never shifts when it lands.
fn gutter(dirty: bool) -> Span<'static> {
    if dirty {
        Span::styled("·", Style::default().fg(theme().pending))
    } else {
        Span::raw(" ")
    }
}

pub fn rows(groups: &[TodoGroup], editing: Editing, hide_done: bool) -> Vec<Row> {
    let mut out = Vec::new();
    let plain = |l: Line<'static>| Row {
        line: l,
        sel: None,
        editing: false,
    };
    let edit_row = |text: &str, cursor: usize, insert: bool, depth: usize| Row {
        line: edit_line_at(text, cursor, insert, depth),
        sel: None,
        editing: true,
    };

    for (gi, g) in groups.iter().enumerate() {
        if gi > 0 {
            out.push(plain(Line::default()));
        }

        // heading: "JROZ-2  Get FraudGen ready…            2 open"
        let mut head = Vec::new();
        match &g.key {
            Some(k) => {
                head.push(Span::styled(
                    k.clone(),
                    Style::default().fg(theme().key).add_modifier(Modifier::BOLD),
                ));
                head.push(Span::raw("  "));
                head.push(Span::styled(g.title.clone(), Style::default().fg(theme().fg)));
            }
            None => head.push(Span::styled(
                g.title.clone(),
                dim().add_modifier(Modifier::BOLD),
            )),
        }
        let open = g.open_count();
        head.push(Span::styled(
            format!("   {open} open"),
            dim(),
        ));
        out.push(plain(Line::from(head)));

        let adding = matches!(editing, Editing::New { gi: g2, .. } if g2 == gi);
        let shown = shown(g, hide_done);

        if shown.is_empty() && !adding {
            // Still a cursor position, so `o` can reach a ticket that has no
            // checkboxes — or none left to show.
            let note = if g.items.is_empty() {
                "(nothing yet — o to add)"
            } else {
                "(all done ✨)"
            };
            out.push(Row {
                line: Line::from(vec![
                    Span::raw("   "),
                    Span::styled(note.to_string(), dim()),
                ]),
                sel: Some(Sel::EmptyGroup(gi)),
                editing: false,
            });
            continue;
        }

        for (ii, it) in shown.iter().map(|&i| (i, &g.items[i])) {
            // an item being reworded shows its buffer in its own place
            if let Editing::Existing { gi: g2, ii: i2, text, cursor, insert } = editing {
                if g2 == gi && i2 == ii {
                    out.push(edit_row(text, cursor, insert, it.depth));
                    continue;
                }
            }
            let (glyph, glyph_style, text_style) = if it.done {
                (
                    "☑",
                    Style::default().fg(theme().done),
                    dim().add_modifier(Modifier::CROSSED_OUT),
                )
            } else {
                (
                    "☐",
                    Style::default().fg(theme().checkbox),
                    Style::default().fg(theme().fg),
                )
            };
            out.push(Row {
                line: Line::from(vec![
                    gutter(it.dirty),
                    Span::raw(" ".repeat(1 + it.depth * 2)),
                    Span::styled(glyph.to_string(), glyph_style),
                    Span::raw(" "),
                    Span::styled(it.text.clone(), text_style),
                ]),
                sel: Some(Sel::Item(gi, ii)),
                editing: false,
            });
        }

        // a new item lands at the end of its group, as an extra row
        if let Editing::New { text, cursor, insert, depth, .. } = editing {
            if adding {
                out.push(edit_row(text, cursor, insert, depth));
            }
        }
    }

    if out.is_empty() {
        out.push(plain(Line::styled(
            "nothing to do ✨".to_string(),
            dim(),
        )));
    }
    out
}

/// Row index of the nth selectable position, for placing the cursor.
pub fn row_of(rows: &[Row], nth: usize) -> Option<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.sel.is_some())
        .map(|(i, _)| i)
        .nth(nth)
}

/// Every position the cursor can occupy, in display order. `ItemList` builds
/// the same sequence from the groups; a test holds the two in step.
pub fn selectables(groups: &[TodoGroup], hide_done: bool) -> Vec<Sel> {
    groups
        .iter()
        .enumerate()
        .flat_map(|(gi, g)| {
            let shown = shown(g, hide_done);
            if shown.is_empty() {
                vec![Sel::EmptyGroup(gi)]
            } else {
                shown.into_iter().map(|ii| Sel::Item(gi, ii)).collect()
            }
        })
        .collect()
}

/// The row being edited: the buffer with a block caret at the cursor, plus a
/// mode marker so you can see at a glance whether typing inserts or commands.
pub fn edit_line_at(text: &str, cursor: usize, insert: bool, depth: usize) -> Line<'static> {
    let mut line = edit_line(text, cursor, insert);
    // keep the caret column lined up with the row it will become
    line.spans.insert(1, Span::raw("  ".repeat(depth)));
    line
}

pub fn edit_line(text: &str, cursor: usize, insert: bool) -> Line<'static> {
    let th = theme();
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars[..cursor.min(chars.len())].iter().collect();
    let at: String = chars.get(cursor).copied().unwrap_or(' ').to_string();
    let after: String = chars
        .get(cursor + 1..)
        .map(|c| c.iter().collect())
        .unwrap_or_default();

    Line::from(vec![
        Span::styled(
            if insert { "❯" } else { "▪" }.to_string(),
            Style::default().fg(th.accent),
        ),
        Span::raw(" ☐ "),
        Span::styled(before, Style::default().fg(th.fg)),
        Span::styled(at, Style::default().fg(th.inverted).bg(th.accent)),
        Span::styled(after, Style::default().fg(th.fg)),
    ])
}

/// The row holding the edit buffer, for scrolling it into view.
pub fn editing_row(rows: &[Row]) -> Option<usize> {
    rows.iter().position(|r| r.editing)
}

pub fn plain_dump(groups: &[TodoGroup]) -> Vec<String> {
    rows(groups, Editing::None, false)
        .iter()
        .map(|r| crate::ui::line_text(&r.line).trim_end().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todo::model::{Origin, TodoItem};

    fn item(text: &str, done: bool) -> TodoItem {
        TodoItem {
            text: text.into(),
            done,
            origin: Origin::Local { line: 0 },
            dirty: false,
            depth: 0,
        }
    }

    fn groups() -> Vec<TodoGroup> {
        vec![
            TodoGroup {
                title: "no ticket".into(),
                key: None,
                items: vec![item("buy milk", false)],
            },
            TodoGroup {
                title: "Get FraudGen ready".into(),
                key: Some("JROZ-2".into()),
                items: vec![item("wire up retry", false), item("fix cache key", true)],
            },
        ]
    }

    #[test]
    fn only_item_rows_are_selectable() {
        let rs = rows(&groups(), Editing::None, false);
        let selectable: Vec<_> = rs.iter().filter_map(|r| r.sel).collect();
        assert_eq!(
            selectable,
            vec![Sel::Item(0, 0), Sel::Item(1, 0), Sel::Item(1, 1)]
        );
    }

    #[test]
    fn row_of_walks_items_not_rows() {
        let rs = rows(&groups(), Editing::None, false);
        // the third item is the ticked one in the second group
        let r = row_of(&rs, 2).unwrap();
        assert_eq!(rs[r].sel, Some(Sel::Item(1, 1)));
    }

    #[test]
    fn done_items_are_ticked_struck_and_quiet() {
        let rs = rows(&groups(), Editing::None, false);
        let done = &rs[row_of(&rs, 2).unwrap()].line;
        let glyph = &done.spans[2];
        assert_eq!(glyph.content, "☑");
        assert_eq!(glyph.style.fg, Some(theme().done));
        let text = done.spans.last().unwrap();
        assert_eq!(text.style.fg, Some(theme().dim));
        assert!(text.style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn the_dirty_gutter_does_not_shift_the_text() {
        let mut gs = groups();
        gs[0].items[0].dirty = true;
        let rs = rows(&gs, Editing::None, false);
        let dirty = &rs[row_of(&rs, 0).unwrap()].line;
        assert_eq!(dirty.spans[0].content, "·");
        assert_eq!(dirty.spans[0].content.chars().count(), 1);
        assert_eq!(dirty.spans[0].style.fg, Some(theme().pending));
    }

    /// Same rule as the ticket pane: the terminal's background must show
    /// through, so only the selected row (painted at draw time) may set one.
    #[test]
    fn nothing_in_the_list_paints_a_background() {
        for (i, r) in rows(&groups(), Editing::None, false).iter().enumerate() {
            assert_eq!(r.line.style.bg, None, "row {i} paints a background");
            for s in &r.line.spans {
                assert_eq!(s.style.bg, None, "a span on row {i} paints a background");
            }
        }
    }
}
