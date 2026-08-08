//! Rendering: ticket → styled lines (Segments), and the per-frame draw.

use crate::adf;
use crate::app::{App, Mode};
use crate::jira::Ticket;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub struct Segments {
    pub lines: Vec<Line<'static>>,
    /// index into `lines` of each comment's header row
    pub comment_headers: Vec<usize>,
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn build(t: &Ticket) -> Segments {
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            t.key.clone(),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(t.itype.clone(), dim()),
        Span::raw("  "),
        Span::styled(t.status.clone(), Style::default().fg(Color::Yellow)),
    ]));
    lines.push(Line::styled(
        t.summary.clone(),
        Style::default().add_modifier(Modifier::BOLD),
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
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
            lines[h].style = lines[h].style.add_modifier(Modifier::REVERSED);
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
            last.spans.push(Span::styled("▌", Style::default().fg(Color::Cyan)));
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
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
            chunks[1],
        );
    }

    // footer: status flash or key hints
    let footer = match &app.status {
        Some(s) => Line::styled(format!(" {s}"), Style::default().fg(Color::Yellow)),
        None => Line::styled(
            " j/k scroll · u/d ½page · gg/G · J/K comment · r refresh · c comment · R reply · w web · q quit",
            dim(),
        ),
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);
}
