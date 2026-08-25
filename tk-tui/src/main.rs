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
mod pick;
mod rest;
mod theme;
mod todo;
mod ui;
mod view;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;
use todo::TodoView;
use view::{Action, View};

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
        if dump {
            for line in TodoView::new()?.dump() {
                println!("{line}");
            }
            return Ok(());
        }
        require_tty("--todo")?;
        let mut terminal = ratatui::init();
        let res = View::todo().and_then(|v| run(&mut terminal, v));
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

    let mut terminal = ratatui::init(); // raw mode + alt screen + panic hook
    let res = View::ticket(&key).and_then(|v| run(&mut terminal, v));
    ratatui::restore();
    res
}

fn require_tty(what: &str) -> Result<()> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        anyhow::bail!("tk-tui needs a TTY — use `tk-tui --dump {what}` for plain output");
    }
    Ok(())
}

/// Drive the view stack. The top view draws and gets the keys; popping the
/// last one exits, which is what makes `tk view KEY` behave as it always did.
fn run(terminal: &mut DefaultTerminal, first: View) -> Result<()> {
    let mut stack = vec![first];
    loop {
        let Some(top) = stack.last_mut() else {
            return Ok(());
        };
        terminal.draw(|f| top.draw(f))?;

        // Poll rather than block, so background writes report back while you
        // keep typing. 100ms is imperceptible and costs nothing when idle.
        if !event::poll(std::time::Duration::from_millis(100))? {
            top.tick();
            continue;
        }
        let Event::Key(k) = event::read()? else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        top.tick();

        // viewport height = frame minus the compose box minus the footer
        let view_h = terminal
            .size()?
            .height
            .saturating_sub(1)
            .saturating_sub(top.compose_height());

        match top.key(k, view_h)? {
            Action::None => {}
            Action::Push(v) => stack.push(*v),
            Action::Pop => {
                stack.pop();
                if stack.is_empty() {
                    return Ok(());
                }
            }
            Action::Quit => return Ok(()),
            // The picker takes the whole terminal, so it happens here rather
            // than inside the view. A failure to start it is a message, not
            // the end of the session.
            Action::Search => {
                let lines = top.search_lines();
                match pick::pick(terminal, "todo", &lines) {
                    Ok(Some(n)) => top.search_pick(n),
                    Ok(None) => {}
                    Err(e) => top.search_failed(format!("{e:#}")),
                }
            }
        }
    }
}
