//! The view stack.
//!
//! `tk view KEY` starts with one ticket on the stack and behaves exactly as it
//! always did — `q` exits, because there's nothing to go back to. `tk todo`
//! starts with the checklist, and Enter on an item pushes that ticket on top;
//! `q` there pops back to the list.

use crate::app::App;
use crate::todo::list::ListAction;
use crate::todo::TodoView;
use anyhow::Result;
use ratatui::crossterm::event::KeyEvent;
use ratatui::Frame;

pub enum Action {
    None,
    Push(Box<View>),
    Pop,
    Quit,
    /// The view wants the fzf picker. It travels to the event loop because
    /// that's what owns the terminal that has to be handed over.
    Search,
}

/// Views are big and hold terminals' worth of state; naming the variant is all
/// a failing assertion needs.
impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Action::None => "None",
            Action::Push(_) => "Push(..)",
            Action::Pop => "Pop",
            Action::Quit => "Quit",
            Action::Search => "Search",
        })
    }
}

pub enum View {
    Todo(Box<TodoView>),
    Ticket(Box<App>),
}

impl View {
    pub fn todo() -> Result<Self> {
        Ok(View::Todo(Box::new(TodoView::new()?)))
    }

    pub fn ticket(key: &str) -> Result<Self> {
        Ok(View::Ticket(Box::new(App::new(key)?)))
    }

    /// A ticket opened from the checklist. It's the ticket pane proper —
    /// description and comments — with `t` still there for its checklist, and
    /// `esc` to step back to the list you came from.
    pub fn ticket_from_list(key: &str) -> Result<Self> {
        let mut app = App::new(key)?;
        app.nested = true;
        Ok(View::Ticket(Box::new(app)))
    }

    pub fn draw(&mut self, f: &mut Frame) {
        match self {
            View::Todo(v) => v.draw(f),
            View::Ticket(a) => crate::ui::draw(f, a),
        }
    }

    /// Rows the view reserves below the body, so the caller can work out the
    /// usable viewport height.
    pub fn compose_height(&self) -> u16 {
        match self {
            View::Todo(_) => 0,
            View::Ticket(a) => crate::ui::compose_height(&a.mode),
        }
    }

    /// Called on every idle poll, for background writes to report back.
    pub fn tick(&mut self) {
        match self {
            View::Todo(v) => v.tick(),
            View::Ticket(a) => a.tick(),
        }
    }

    /// The checklist this view is showing, if it's showing one. Both views
    /// search through the same widget — the aggregate list, or the one ticket's
    /// — so neither needs its own idea of what searching means.
    fn list(&mut self) -> Option<&mut crate::todo::list::ItemList> {
        match self {
            View::Todo(v) => Some(&mut v.list),
            View::Ticket(a) if a.focus == crate::app::Focus::Todo => Some(&mut a.todo),
            View::Ticket(_) => None,
        }
    }

    pub fn search_lines(&mut self) -> Vec<String> {
        self.list().map(|l| l.search_lines()).unwrap_or_default()
    }

    pub fn search_pick(&mut self, nth: usize) {
        if let Some(l) = self.list() {
            l.search_pick(nth);
        }
    }

    /// Say why the picker didn't run, where the user is looking.
    pub fn search_failed(&mut self, why: String) {
        if let Some(l) = self.list() {
            l.status = Some(why);
        }
    }

    pub fn key(&mut self, k: KeyEvent, view_h: u16) -> Result<Action> {
        Ok(match self {
            View::Todo(v) => match v.key(k, view_h) {
                ListAction::Quit | ListAction::Back => Action::Pop,
                ListAction::Open(key) => match View::ticket_from_list(&key) {
                    Ok(view) => Action::Push(Box::new(view)),
                    Err(e) => {
                        v.list.status = Some(format!("can't open {key}: {e:#}"));
                        Action::None
                    }
                },
                ListAction::Search => Action::Search,
                ListAction::Reload | ListAction::None => Action::None,
            },
            View::Ticket(a) => a.key(k, view_h),
        })
    }
}
