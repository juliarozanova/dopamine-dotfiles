//! Rendering: ticket → styled lines (Segments), and the per-frame draw.

use crate::adf;
use crate::app::{App, Focus, Mode};
use crate::jira::Ticket;
use crate::theme::theme;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub struct Segments {
    pub lines: Vec<Line<'static>>,
    /// index into `lines` of each comment's header row
    pub comment_headers: Vec<usize>,
}

fn dim() -> Style {
    Style::default().fg(theme().dim)
}

pub fn build(t: &Ticket) -> Segments {
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            t.key.clone(),
            Style::default().fg(theme().key).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(t.itype.clone(), dim()),
        Span::raw("  "),
        Span::styled(t.status.clone(), Style::default().fg(theme().status)),
    ]));
    lines.push(Line::styled(
        t.summary.clone(),
        Style::default().fg(theme().fg).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::default());

    match &t.description {
        Some(d) => lines.extend(adf::to_lines(d)),
        None => lines.push(Line::styled("(no description)".to_string(), dim())),
    }
    lines.push(Line::default());

    let n = t.comments.len();
    lines.push(Line::styled(
        format!("── {n} comment{} ──", if n == 1 { "" } else { "s" }),
        dim(),
    ));

    let mut comment_headers = Vec::new();
    for c in &t.comments {
        lines.push(Line::default());
        comment_headers.push(lines.len());
        lines.push(Line::from(vec![
            Span::styled(
                format!("◆ {}", c.author),
                Style::default().fg(theme().author).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {}", c.created), dim()),
        ]));
        for mut l in adf::to_lines(&c.body) {
            l.spans.insert(0, Span::raw("  "));
            lines.push(l);
        }
    }

    Segments {
        lines,
        comment_headers,
    }
}

pub fn line_text(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn plain_dump(t: &Ticket) -> Vec<String> {
    build(t).lines.iter().map(line_text).collect()
}

pub fn compose_height(mode: &Mode) -> u16 {
    match mode {
        Mode::Compose { .. } => 7,
        Mode::Normal => 0,
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(compose_height(&app.mode)),
        Constraint::Length(1),
    ])
    .split(f.area());

    // TODO mode: the same checklist widget as `tk todo`, over this one ticket
    if app.focus == Focus::Todo {
        let head = Line::from(vec![
            Span::styled(
                app.key.clone(),
                Style::default().fg(theme().key).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  TODO".to_string(), dim()),
        ]);
        let body = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(chunks[0]);
        f.render_widget(Paragraph::new(head), body[0]);
        app.todo.draw(f, body[1]);

        let footer = match &app.todo.status {
            Some(s) => Line::styled(format!(" {s}"), Style::default().fg(theme().status)),
            None => Line::styled(
                app.todo.hint().replace("q quit", "esc ticket · q quit"),
                dim(),
            ),
        };
        f.render_widget(Paragraph::new(footer), chunks[2]);
        return;
    }

    // body, with the selected comment header reversed
    let mut lines = app.segs.lines.clone();
    if let Some(s) = app.sel {
        if let Some(&h) = app.segs.comment_headers.get(s) {
            lines[h].style = lines[h]
                .style
                .bg(theme().selection)
                .fg(theme().inverted);
        }
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        chunks[0],
    );

    // compose box
    if let Mode::Compose { reply_to } = &app.mode {
        let insert = app.input.mode() == crate::editor::EditMode::Insert;
        let how = if insert { "esc normal · ZZ send" } else { "ZZ send · ZQ discard" };
        let title = match reply_to {
            Some(i) => format!(
                " reply to {} — {how} ",
                app.ticket.comments[*i].author
            ),
            None => format!(" comment on {} — {how} ", app.key),
        };
        let buf = app.input.text();
        let mut text: Vec<Line> = buf.split('\n').map(|l| Line::raw(l.to_string())).collect();
        // place the caret on the line the cursor is actually in
        let (caret_line, caret_col) = {
            let mut remaining = app.input.cursor();
            let mut row = 0;
            for (i, l) in buf.split('\n').enumerate() {
                let n = l.chars().count();
                row = i;
                if remaining <= n {
                    break;
                }
                remaining -= n + 1;
            }
            (row, remaining)
        };
        if let Some(line) = text.get_mut(caret_line) {
            let chars: Vec<char> = line_text(line).chars().collect();
            let before: String = chars[..caret_col.min(chars.len())].iter().collect();
            let at: String = chars.get(caret_col).copied().unwrap_or(' ').to_string();
            let after: String = chars
                .get(caret_col + 1..)
                .map(|c| c.iter().collect())
                .unwrap_or_default();
            *line = Line::from(vec![
                Span::raw(before),
                Span::styled(at, Style::default().fg(theme().inverted).bg(theme().accent)),
                Span::raw(after),
            ]);
        }
        // keep the last lines visible in the fixed-height box
        let inner_h = chunks[1].height.saturating_sub(2) as usize;
        let skip = text.len().saturating_sub(inner_h.max(1));
        let text: Vec<Line> = text.into_iter().skip(skip).collect();
        f.render_widget(Clear, chunks[1]);
        f.render_widget(
            Paragraph::new(text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(theme().accent)),
            ),
            chunks[1],
        );
    }

    // footer: status flash or key hints
    // The way back leads, because the footer is longer than most panes are
    // wide and the tail is the first thing a narrow pane loses.
    let back = if app.nested { "esc todo list · " } else { "" };
    let footer = match &app.status {
        Some(s) => Line::styled(format!(" {s}"), Style::default().fg(theme().status)),
        None => Line::styled(
            format!(
                " {back}t todo · j/k scroll · u/d ½page · gg/G · J/K comment · r refresh · c comment · R reply · w web · q quit"
            ),
            dim(),
        ),
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::Comment;
    use serde_json::json;

    pub(super) fn ticket_fixture() -> Ticket {
        Ticket {
            key: "FRD-123".into(),
            summary: "a summary".into(),
            status: "In Progress".into(),
            itype: "Task".into(),
            description: Some(json!({ "type": "doc", "content": [
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "inline", "marks": [{ "type": "code" }] },
                    { "type": "text", "text": "linked", "marks": [{ "type": "link" }] }
                ]},
                { "type": "codeBlock", "content": [{ "type": "text", "text": "let x = 1;" }] },
                { "type": "heading", "attrs": { "level": 2 },
                  "content": [{ "type": "text", "text": "TODO" }] },
                { "type": "taskList", "attrs": { "localId": "tl" }, "content": [
                    { "type": "taskItem", "attrs": { "localId": "a1", "state": "TODO" },
                      "content": [{ "type": "text", "text": "an open task" }] },
                    { "type": "taskItem", "attrs": { "localId": "b2", "state": "DONE" },
                      "content": [{ "type": "text", "text": "a finished task" }] }
                ]},
                { "type": "rule" }
            ]})),
            comments: vec![Comment {
                author: "Ada".into(),
                created: "2026-08-01 11:41".into(),
                body: json!({ "type": "doc", "content": [] }),
            }],
        }
    }

    fn find(segs: &Segments, needle: &str) -> Span<'static> {
        segs.lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("no span containing {needle:?}"))
            .clone()
    }

    /// `Line::styled` carries its colour on the line, not on the inner span.
    fn find_line(segs: &Segments, needle: &str) -> Line<'static> {
        segs.lines
            .iter()
            .find(|l| line_text(l).contains(needle))
            .unwrap_or_else(|| panic!("no line containing {needle:?}"))
            .clone()
    }

    #[test]
    fn header_and_comment_styles_come_from_the_theme() {
        let segs = build(&ticket_fixture());
        let th = theme();

        let header = &segs.lines[0];
        assert_eq!(header.spans[0].content, "FRD-123");
        assert_eq!(header.spans[0].style.fg, Some(th.key));
        assert!(header.spans[0].style.add_modifier.contains(Modifier::BOLD));

        let status = header.spans.last().unwrap();
        assert_eq!(status.content, "In Progress");
        assert_eq!(status.style.fg, Some(th.status));

        let author_row = &segs.lines[segs.comment_headers[0]];
        assert_eq!(author_row.spans[0].style.fg, Some(th.author));
        // the timestamp beside it stays quiet
        assert_eq!(author_row.spans[1].style.fg, Some(th.dim));
    }

    #[test]
    fn adf_marks_come_from_the_theme() {
        let segs = build(&ticket_fixture());
        let th = theme();

        assert_eq!(find(&segs, "inline").style.fg, Some(th.code_inline));
        assert_eq!(find(&segs, "linked").style.fg, Some(th.link));
        assert_eq!(find(&segs, "let x = 1;").style.fg, Some(th.code));
        assert_eq!(find_line(&segs, "────────").style.fg, Some(th.rule));
        // the "── N comments ──" divider is the quiet colour, not the rule colour
        assert_eq!(find_line(&segs, "comment ──").style.fg, Some(th.dim));
    }

    /// A checkbox in the description reads as a checkbox in the pane, not as
    /// bare text — the same nodes the checklist is built from.
    #[test]
    fn task_items_render_as_checkboxes() {
        let segs = build(&ticket_fixture());
        let th = theme();

        let open = find_line(&segs, "an open task");
        assert!(line_text(&open).starts_with("☐ "));
        assert_eq!(open.spans[1].style.fg, Some(th.checkbox));

        let done = find_line(&segs, "a finished task");
        assert!(line_text(&done).starts_with("☑ "));
        assert_eq!(done.spans[1].style.fg, Some(th.done));
        assert!(
            done.spans[2].style.add_modifier.contains(Modifier::CROSSED_OUT),
            "a finished task reads as finished"
        );
    }

    /// The pane must never paint a backdrop, or WezTerm's
    /// window_background_opacity can't show through — the same trap nvim hit.
    #[test]
    fn nothing_in_the_body_paints_a_background() {
        let segs = build(&ticket_fixture());
        for (i, line) in segs.lines.iter().enumerate() {
            assert_eq!(line.style.bg, None, "line {i} paints a background");
            for span in &line.spans {
                assert_eq!(span.style.bg, None, "a span on line {i} paints a background");
            }
        }
    }
}

/// Frame-level tests for the pane itself. `t` used to flip a field that
/// nothing rendered, so the mode existed only in the state.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::app::Focus;
    use crate::editor::{EditMode, Editor};
    use crate::todo::list::ItemList;
    use crate::todo::model::{Origin, TodoGroup, TodoItem};
    use crate::ui::build;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> App {
        let ticket = super::tests::ticket_fixture();
        let segs = build(&ticket);
        let mut todo = ItemList::new(None);
        todo.groups = vec![TodoGroup {
            title: ticket.summary.clone(),
            key: Some("FRD-123".into()),
            items: vec![TodoItem {
                text: "an open task".into(),
                done: false,
                origin: Origin::Jira {
                    key: "FRD-123".into(),
                    local_id: "a1".into(),
                },
                dirty: false,
            }],
        }];
        App {
            key: "FRD-123".into(),
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
            todo,
        }
    }

    fn screen(app: &mut App) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(70, 16)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn t_actually_swaps_the_pane_for_the_checklist() {
        let mut a = app();

        let full = screen(&mut a).join("\n");
        assert!(full.contains("a summary"), "the ticket body is showing");
        assert!(full.contains("comment"), "…and its comments");

        a.key(
            ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char('t'),
                ratatui::crossterm::event::KeyModifiers::NONE,
            ),
            14,
        );
        let todo = screen(&mut a).join("\n");
        assert!(todo.contains("an open task"), "the checklist is showing:\n{todo}");
        assert!(!todo.contains("comment"), "the comments are not:\n{todo}");
        assert!(todo.contains("TODO"), "and it says what it is");

        // and back again, with the description position kept
        a.key(
            ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char('t'),
                ratatui::crossterm::event::KeyModifiers::NONE,
            ),
            14,
        );
        assert!(screen(&mut a).join("\n").contains("a summary"));
    }

    /// The footer is longer than a floating pane is wide, so anything that
    /// matters has to survive truncation at a realistic width.
    #[test]
    fn the_footer_advertises_the_todo_toggle() {
        let mut a = app();
        let footer = screen(&mut a).last().unwrap().clone();
        assert!(footer.contains("t todo"), "hotkey hints must mention it: {footer:?}");
    }

    /// Enter from the checklist has to leave you a way home, and the footer
    /// has to say what it is.
    #[test]
    fn a_nested_ticket_advertises_the_way_back() {
        let mut a = app();
        assert!(
            !screen(&mut a).last().unwrap().contains("esc todo list"),
            "a standalone `tk view` has nowhere to go back to"
        );
        a.nested = true;
        assert!(screen(&mut a).last().unwrap().contains("esc todo list"));
    }

    #[test]
    fn esc_steps_back_but_q_closes_the_pane() {
        use crate::view::Action;
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        let mut a = app();
        assert!(matches!(a.key(key(KeyCode::Esc), 14), Action::Pop));

        let mut a = app();
        assert!(matches!(a.key(key(KeyCode::Char('q')), 14), Action::Quit));

        // and from the ticket's own checklist, esc returns to the ticket
        let mut a = app();
        a.focus = Focus::Todo;
        assert!(matches!(a.key(key(KeyCode::Esc), 14), Action::None));
        assert_eq!(a.focus, Focus::Full);
    }

    #[test]
    fn the_checklist_footer_replaces_the_ticket_hints() {
        let mut a = app();
        a.focus = Focus::Todo;
        let footer = screen(&mut a).last().unwrap().clone();
        assert!(footer.contains("space done"), "checklist hints: {footer:?}");
    }
}
