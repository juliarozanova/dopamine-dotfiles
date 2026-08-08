//! tk-tui — ratatui Jira ticket pane for the tk workflow.
//!
//!   tk-tui ISSUE-KEY          interactive pane
//!   tk-tui --dump ISSUE-KEY   plain-text render to stdout (for scripts/tests)
//!
//! Launch via `tk view` so JIRA_API_TOKEN is freshly sourced.

mod adf;
mod app;
mod jira;
mod ui;

use anyhow::Result;
use app::{App, Mode};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

fn main() -> Result<()> {
    let mut key: Option<String> = None;
    let mut dump = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--dump" => dump = true,
            "-h" | "--help" => {
                println!("usage: tk-tui [--dump] ISSUE-KEY");
                return Ok(());
            }
            other => key = Some(other.to_string()),
        }
    }
    let key = key.ok_or_else(|| anyhow::anyhow!("usage: tk-tui [--dump] ISSUE-KEY"))?;

    if dump {
        let t = jira::fetch(&key)?;
        for line in ui::plain_dump(&t) {
            println!("{line}");
        }
        return Ok(());
    }

    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        anyhow::bail!("tk-tui needs a TTY — use `tk-tui --dump {key}` for plain output");
    }

    let mut app = App::new(&key)?;
    let mut terminal = ratatui::init(); // raw mode + alt screen + panic hook
    let res = run(&mut terminal, &mut app);
    ratatui::restore();
    res
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        let Event::Key(k) = event::read()? else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        // any keypress clears the last status flash
        app.status = None;

        // viewport height = frame minus compose box minus footer
        let view_h = terminal
            .size()?
            .height
            .saturating_sub(1)
            .saturating_sub(ui::compose_height(&app.mode));
        let half = (view_h / 2).max(1) as i32;

        match app.mode {
            Mode::Normal => {
                let g = std::mem::take(&mut app.pending_g);
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match (k.code, ctrl) {
                    (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return Ok(()),
                    (KeyCode::Char('c'), true) => return Ok(()),

                    (KeyCode::Char('j'), false) | (KeyCode::Down, _) => app.scroll_by(1, view_h),
                    (KeyCode::Char('k'), false) | (KeyCode::Up, _) => app.scroll_by(-1, view_h),
                    (KeyCode::Char('d'), _) => app.scroll_by(half, view_h),
                    (KeyCode::Char('u'), _) => app.scroll_by(-half, view_h),
                    (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => {
                        app.scroll_by(view_h as i32, view_h)
                    }
                    (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => {
                        app.scroll_by(-(view_h as i32), view_h)
                    }
                    (KeyCode::Char('g'), false) => {
                        if g {
                            app.scroll = 0;
                        } else {
                            app.pending_g = true;
                        }
                    }
                    (KeyCode::Char('G'), _) | (KeyCode::End, _) => {
                        app.scroll = app.max_scroll(view_h)
                    }
                    (KeyCode::Home, _) => app.scroll = 0,

                    // comment navigation: J/K, n/p, and l/h all work
                    (KeyCode::Char('J'), _)
                    | (KeyCode::Char('n'), false)
                    | (KeyCode::Char('l'), false) => app.select_comment(1, view_h),
                    (KeyCode::Char('K'), _)
                    | (KeyCode::Char('p'), false)
                    | (KeyCode::Char('h'), false) => app.select_comment(-1, view_h),

                    (KeyCode::Char('r'), false) => app.reload(),
                    (KeyCode::Char('w'), false) => {
                        jira::open_in_browser(&app.key);
                        app.status = Some("opening in browser…".into());
                    }
                    (KeyCode::Char('c'), false) => {
                        app.mode = Mode::Compose { reply_to: None };
                    }
                    (KeyCode::Char('R'), _) => match app.sel {
                        Some(i) => app.mode = Mode::Compose { reply_to: Some(i) },
                        None => {
                            app.status =
                                Some("select a comment first (J/K), then R to reply".into())
                        }
                    },
                    _ => {}
                }
            }
            Mode::Compose { .. } => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match (k.code, ctrl) {
                    (KeyCode::Esc, _) => {
                        app.mode = Mode::Normal;
                        app.status = Some("draft kept — c/R to continue".into());
                    }
                    (KeyCode::Char('s'), true) => app.submit(),
                    (KeyCode::Enter, _) => app.input.push('\n'),
                    (KeyCode::Backspace, _) => {
                        app.input.pop();
                    }
                    (KeyCode::Char(ch), false) => app.input.push(ch),
                    _ => {}
                }
            }
        }
    }
}
