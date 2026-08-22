//! Rendering: ticket → styled lines (Segments), and the per-frame draw.

use crate::adf;
use crate::app::{App, Mode};
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

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(compose_height(&app.mode)),
        Constraint::Length(1),
    ])
    .split(f.area());

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
        let title = match reply_to {
            Some(i) => format!(
                " reply to {} — ctrl-s send · esc cancel ",
                app.ticket.comments[*i].author
            ),
            None => format!(" comment on {} — ctrl-s send · esc cancel ", app.key),
        };
        let mut text: Vec<Line> = app.input.split('\n').map(Line::raw).collect();
        if let Some(last) = text.last_mut() {
            last.spans.push(Span::styled("▌", Style::default().fg(theme().accent)));
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
    let footer = match &app.status {
        Some(s) => Line::styled(format!(" {s}"), Style::default().fg(theme().status)),
        None => Line::styled(
            " j/k scroll · u/d ½page · gg/G · J/K comment · r refresh · c comment · R reply · w web · q quit",
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

    fn ticket() -> Ticket {
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
        let segs = build(&ticket());
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
        let segs = build(&ticket());
        let th = theme();

        assert_eq!(find(&segs, "inline").style.fg, Some(th.code_inline));
        assert_eq!(find(&segs, "linked").style.fg, Some(th.link));
        assert_eq!(find(&segs, "let x = 1;").style.fg, Some(th.code));
        assert_eq!(find_line(&segs, "────────").style.fg, Some(th.rule));
        // the "── N comments ──" divider is the quiet colour, not the rule colour
        assert_eq!(find_line(&segs, "comment ──").style.fg, Some(th.dim));
    }

    /// The pane must never paint a backdrop, or WezTerm's
    /// window_background_opacity can't show through — the same trap nvim hit.
    #[test]
    fn nothing_in_the_body_paints_a_background() {
        let segs = build(&ticket());
        for (i, line) in segs.lines.iter().enumerate() {
            assert_eq!(line.style.bg, None, "line {i} paints a background");
            for span in &line.spans {
                assert_eq!(span.style.bg, None, "a span on line {i} paints a background");
            }
        }
    }
}
