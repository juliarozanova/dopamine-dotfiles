//! tk-tui — ratatui Jira ticket pane for the tk workflow.
//!
//!   tk-tui ISSUE-KEY          interactive ticket pane
//!   tk-tui --todo             the aggregate checklist
//!   tk-tui --dump [KEY]       plain-text render to stdout (for scripts/tests)
//!
//! Launch via `tk view` / `tk todo` so JIRA_API_TOKEN is freshly sourced.

mod adf;
mod app;
mod editor;
mod jira;
mod rest;
mod theme;
mod todo;
mod ui;

use anyhow::Result;
use app::{App, Mode};
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::DefaultTerminal;
use todo::TodoView;

const USAGE: &str = "usage: tk-tui [--dump] ISSUE-KEY  |  tk-tui [--dump] --todo";

fn main() -> Result<()> {
    let mut key: Option<String> = None;
    let mut dump = false;
    let mut todo_mode = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--dump" => dump = true,
            "--todo" => todo_mode = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => key = Some(other.to_string()),
        }
    }

    if todo_mode {
        let mut view = TodoView::new()?;
        if dump {
            for line in view.dump() {
                println!("{line}");
            }
            return Ok(());
        }
        require_tty("--todo")?;
        let mut terminal = ratatui::init();
        let res = run_todo(&mut terminal, &mut view);
        ratatui::restore();
        return res;
    }

    let key = key.ok_or_else(|| anyhow::anyhow!("{USAGE}"))?;

    if dump {
        let t = jira::fetch(&key)?;
        for line in ui::plain_dump(&t) {
            println!("{line}");
        }
        return Ok(());
    }

    require_tty(&key)?;

    let mut app = App::new(&key)?;
    let mut terminal = ratatui::init(); // raw mode + alt screen + panic hook
    let res = run(&mut terminal, &mut app);
    ratatui::restore();
    res
}

fn require_tty(what: &str) -> Result<()> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        anyhow::bail!("tk-tui needs a TTY — use `tk-tui --dump {what}` for plain output");
    }
    Ok(())
}

fn run_todo(terminal: &mut DefaultTerminal, view: &mut TodoView) -> Result<()> {
    loop {
        terminal.draw(|f| view.draw(f))?;

        // Poll rather than block, so background writes can report back while
        // you keep typing. 100ms is imperceptible and costs nothing when idle.
        if !event::poll(std::time::Duration::from_millis(100))? {
            view.poll_sync();
            continue;
        }
        let Event::Key(k) = event::read()? else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        view.poll_sync();

        // While an item is open for editing the editor owns every key.
        if view.editing() && view.edit_key(k) {
            continue;
        }
        view.status = None;

        let view_h = terminal.size()?.height.saturating_sub(1);
        let half = (view_h / 2).max(1) as i32;
        let g = std::mem::take(&mut view.pending_g);
        let d = std::mem::take(&mut view.pending_d);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

        match (k.code, ctrl) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return Ok(()),
            (KeyCode::Char('c'), true) => return Ok(()),

            (KeyCode::Char('j'), false) | (KeyCode::Down, _) => view.move_cursor(1),
            (KeyCode::Char('k'), false) | (KeyCode::Up, _) => view.move_cursor(-1),
            (KeyCode::Char('d'), true) => view.move_cursor(half),
            (KeyCode::Char('u'), true) => view.move_cursor(-half),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => view.move_cursor(view_h as i32),
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => view.move_cursor(-(view_h as i32)),
            (KeyCode::Char('g'), false) => {
                if g {
                    view.cursor = 0;
                } else {
                    view.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) | (KeyCode::End, _) => {
                view.cursor = view.item_count().saturating_sub(1)
            }
            (KeyCode::Home, _) => view.cursor = 0,

            (KeyCode::Char('J'), _) => view.move_group(1),
            (KeyCode::Char('K'), _) => view.move_group(-1),

            (KeyCode::Char(' '), _) => view.toggle(),
            (KeyCode::Char('r'), false) => view.reload(),

            // editing
            (KeyCode::Char('i'), false) | (KeyCode::Enter, _) => {
                view.edit_item(editor::EditMode::Insert)
            }
            (KeyCode::Char('A'), _) => view.edit_item(editor::EditMode::Insert),
            (KeyCode::Char('e'), false) => view.edit_item(editor::EditMode::Normal),
            (KeyCode::Char('c'), false) => {
                // cc — replace the item's text
                view.edit_item(editor::EditMode::Normal);
                view.edit_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
                view.edit_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
            }
            (KeyCode::Char('o'), false) => view.new_item(),
            (KeyCode::Char('d'), false) => {
                if d {
                    view.delete_item();
                } else {
                    view.pending_d = true;
                }
            }
            _ => {}
        }
    }
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
                // Enter inserts a newline in a comment, so the editor's own
                // Enter-commits rule doesn't apply here: ZZ sends, ZQ drops.
                if k.code == KeyCode::Enter {
                    app.input.key(KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::NONE));
                    continue;
                }
                match app.input.key(k) {
                    editor::Outcome::Continue => {}
                    editor::Outcome::Commit => app.submit(),
                    editor::Outcome::Cancel => {
                        app.mode = Mode::Normal;
                        app.status = Some("draft kept — c/R to continue".into());
                    }
                }
            }
        }
    }
}
