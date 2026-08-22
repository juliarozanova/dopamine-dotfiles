//! Rendering for the checklist. Shared by the aggregate todo view and the
//! ticket pane's TODO mode, so the two can never drift apart.

use crate::theme::theme;
use crate::todo::model::TodoGroup;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// One rendered row. `item` is the (group, item) it belongs to, or None for
/// headings and spacers — which is what keeps the cursor on real items only.
pub struct Row {
    pub line: Line<'static>,
    pub item: Option<(usize, usize)>,
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

pub fn rows(groups: &[TodoGroup]) -> Vec<Row> {
    let mut out = Vec::new();
    let plain = |l: Line<'static>| Row { line: l, item: None };

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

        if g.items.is_empty() {
            out.push(plain(Line::from(vec![
                Span::raw("   "),
                Span::styled("(nothing yet — o to add)".to_string(), dim()),
            ])));
            continue;
        }

        for (ii, it) in g.items.iter().enumerate() {
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
                    Span::raw(" "),
                    Span::styled(glyph.to_string(), glyph_style),
                    Span::raw(" "),
                    Span::styled(it.text.clone(), text_style),
                ]),
                item: Some((gi, ii)),
            });
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

/// Row index of the nth item overall, for scrolling the cursor into view.
pub fn row_of(rows: &[Row], nth: usize) -> Option<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.item.is_some())
        .map(|(i, _)| i)
        .nth(nth)
}

pub fn plain_dump(groups: &[TodoGroup]) -> Vec<String> {
    rows(groups)
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
        let rs = rows(&groups());
        let selectable: Vec<_> = rs.iter().filter_map(|r| r.item).collect();
        assert_eq!(selectable, vec![(0, 0), (1, 0), (1, 1)]);
    }

    #[test]
    fn row_of_walks_items_not_rows() {
        let rs = rows(&groups());
        // the third item is the ticked one in the second group
        let r = row_of(&rs, 2).unwrap();
        assert_eq!(rs[r].item, Some((1, 1)));
    }

    #[test]
    fn done_items_are_ticked_struck_and_quiet() {
        let rs = rows(&groups());
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
        let rs = rows(&gs);
        let dirty = &rs[row_of(&rs, 0).unwrap()].line;
        assert_eq!(dirty.spans[0].content, "·");
        assert_eq!(dirty.spans[0].content.chars().count(), 1);
        assert_eq!(dirty.spans[0].style.fg, Some(theme().pending));
    }

    /// Same rule as the ticket pane: the terminal's background must show
    /// through, so only the selected row (painted at draw time) may set one.
    #[test]
    fn nothing_in_the_list_paints_a_background() {
        for (i, r) in rows(&groups()).iter().enumerate() {
            assert_eq!(r.line.style.bg, None, "row {i} paints a background");
            for s in &r.line.spans {
                assert_eq!(s.style.bg, None, "a span on row {i} paints a background");
            }
        }
    }
}
