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

    /// A ticket opened from the checklist lands in TODO mode — you clicked
    /// through from a task, so the tasks are what you want to see first.
    pub fn ticket_todo(key: &str) -> Result<Self> {
        let mut app = App::new(key)?;
        app.show_todo();
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

    pub fn key(&mut self, k: KeyEvent, view_h: u16) -> Result<Action> {
        Ok(match self {
            View::Todo(v) => match v.key(k, view_h) {
                ListAction::Quit => Action::Pop,
                ListAction::Open(key) => match View::ticket_todo(&key) {
                    Ok(view) => Action::Push(Box::new(view)),
                    Err(e) => {
                        v.list.status = Some(format!("can't open {key}: {e:#}"));
                        Action::None
                    }
                },
                ListAction::Reload | ListAction::None => Action::None,
            },
            View::Ticket(a) => a.key(k, view_h),
        })
    }
}
