//! Handing the terminal over to fzf and getting a choice back.
//!
//! tk already picks tickets and repos with fzf (`executable_tk`), so the
//! checklist does too rather than growing a fuzzy matcher of its own: same
//! query language, same keys, and nothing here to get subtly wrong.
//!
//! Kept general — it takes lines and returns an index — because the parked
//! issue-list mode wants exactly this and shouldn't have to reinvent it.

use anyhow::{Context, Result};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::DefaultTerminal;
use std::io::Write;
use std::process::{Command, Stdio};

/// The vim motions every tk picker has, verbatim from `executable_tk`. Plain
/// `j`/`k` stay literal — you still have to be able to type a query.
const VIM_BINDS: &str = "ctrl-j:down,ctrl-k:up,ctrl-d:half-page-down,\
                         ctrl-u:half-page-up,ctrl-f:page-down,ctrl-b:page-up";

/// Suspend the pane, run fzf over `lines`, and return the index chosen.
///
/// `None` covers every ordinary way of not choosing — escape, no match, an
/// empty list — so callers only handle errors that are actually errors.
pub fn pick(term: &mut DefaultTerminal, prompt: &str, lines: &[String]) -> Result<Option<usize>> {
    if lines.is_empty() {
        return Ok(None);
    }
    let mut cmd = Command::new("fzf");
    cmd.args([
        &format!("--prompt={prompt}> "),
        "--reverse",
        "--delimiter=\t",
        // The index rides along in field 1, hidden from the display *and*
        // from matching, so two identically worded todos can't be confused
        // for each other.
        "--with-nth=2..",
        "--bind",
        VIM_BINDS,
    ]);

    // Leave our alternate screen before fzf enters its own, and undo it in
    // the same order on the way back. Without --height fzf takes the whole
    // screen and restores it on exit, so nothing of it is left behind.
    suspended(term, || run(cmd, lines))
}

/// Drop out of the TUI for the duration of `f`, whatever `f` does.
fn suspended<T>(term: &mut DefaultTerminal, f: impl FnOnce() -> Result<T>) -> Result<T> {
    disable_raw_mode().context("leaving raw mode")?;
    execute!(std::io::stdout(), LeaveAlternateScreen).context("leaving the alternate screen")?;

    let out = f();

    execute!(std::io::stdout(), EnterAlternateScreen).context("re-entering the alternate screen")?;
    enable_raw_mode().context("re-entering raw mode")?;
    // ratatui draws by diffing against the buffer it last drew. The screen
    // underneath is now blank, so without this only the cells that happen to
    // have changed would be repainted and the rest would stay empty.
    term.clear().context("repainting")?;
    term.hide_cursor().ok();
    out
}

/// The pipe plumbing, with the command injectable so a test can substitute a
/// picker that needs no terminal.
fn run(mut cmd: Command, lines: &[String]) -> Result<Option<usize>> {
    // stderr is inherited: fzf draws on the tty, not down the pipe.
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("fzf not found — install it (brew install fzf / apt install fzf)")
        }
        Err(e) => return Err(e).context("starting the picker"),
    };

    let mut stdin = child.stdin.take().context("the picker took no stdin")?;
    let payload: String = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{i}\t{l}\n"))
        .collect();
    // On its own thread: a long enough list fills the pipe buffer, and then
    // writing and waiting in the same thread is a deadlock.
    let writer = std::thread::spawn(move || stdin.write_all(payload.as_bytes()));

    let out = child.wait_with_output().context("waiting for the picker")?;
    // A picker that exits before reading everything gives the writer EPIPE.
    // That is not a failure — it's what "I've chosen, stop" looks like.
    let _ = writer.join();

    if !out.status.success() {
        // 1 = nothing matched, 130 = you pressed escape. Both are answers.
        return Ok(None);
    }
    Ok(index_of(&String::from_utf8_lossy(&out.stdout)))
}

/// The chosen line's index, from the field we hid in front of it.
fn index_of(selection: &str) -> Option<usize> {
    selection
        .lines()
        .next()?
        .split('\t')
        .next()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for fzf: `sed -n '<n>p'` picks the nth line and exits, which
    /// exercises the same pipes, the same exit status and the same parse.
    fn sed(n: usize) -> Command {
        let mut c = Command::new("sed");
        c.args(["-n", &format!("{n}p")]);
        c
    }

    fn lines() -> Vec<String> {
        vec!["alpha".into(), "beta".into(), "gamma".into()]
    }

    #[test]
    fn the_index_survives_the_round_trip_through_the_picker() {
        assert_eq!(run(sed(3), &lines()).unwrap(), Some(2), "third line, index 2");
        assert_eq!(run(sed(1), &lines()).unwrap(), Some(0));
    }

    #[test]
    fn choosing_nothing_is_not_an_error() {
        // sed with no matching line prints nothing and still exits 0
        assert_eq!(run(sed(99), &lines()).unwrap(), None);
        // a picker that exits non-zero is the escape key, not a failure
        let mut no = Command::new("false");
        no.arg("");
        assert_eq!(run(no, &lines()).unwrap(), None);
    }

    #[test]
    fn a_missing_picker_says_so_instead_of_panicking() {
        let e = run(Command::new("definitely-not-a-real-binary-xyz"), &lines())
            .expect_err("should be an error");
        assert!(format!("{e:#}").contains("fzf not found"), "got {e:#}");
    }

    #[test]
    fn the_hidden_index_is_not_part_of_what_you_see() {
        // field 1 is the index, and --with-nth=2.. keeps it off the screen and
        // out of the matching — so this is the shape the flag depends on
        assert_eq!(index_of("2\tJROZ-2  ☐ wire up retry"), Some(2));
        assert_eq!(index_of("2\tJROZ-2  ☐ wire up retry\n"), Some(2));
        // a line fzf never produced shouldn't move the cursor anywhere
        assert_eq!(index_of("no tab here"), None);
        assert_eq!(index_of(""), None);
    }

    #[test]
    fn an_empty_list_never_starts_a_picker() {
        // pick() short-circuits before touching the terminal; run() would
        // otherwise hand fzf nothing to choose from
        assert_eq!(run(sed(1), &[]).unwrap(), None);
    }
}
