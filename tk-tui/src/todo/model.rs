//! The format-neutral todo model.
//!
//! Backends parse into this and take mutations back out; nothing above this
//! layer knows whether an item came from a markdown line or an ADF taskItem.
//! That's what lets one checklist widget and one modal editor serve both.

/// Where an item lives, and how to find it again on write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A `taskItem` in a ticket description. `local_id` is ADF's own stable
    /// identity — it survives text edits, which is why write-back can be
    /// surgical rather than a re-render of the whole description.
    Jira { key: String, local_id: String },
    /// A checkbox line in the local markdown file, by line index.
    Local { line: usize },
}

impl Origin {
    pub fn ticket(&self) -> Option<&str> {
        match self {
            Origin::Jira { key, .. } => Some(key),
            Origin::Local { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub origin: Origin,
    /// True between an optimistic local change and its write landing.
    pub dirty: bool,
}

#[derive(Clone, Debug)]
pub struct TodoGroup {
    /// Rendered heading, e.g. "JROZ-2  Get FraudGen ready for baseline testing".
    pub title: String,
    /// The ticket this group came from; `None` is the local file.
    pub key: Option<String>,
    pub items: Vec<TodoItem>,
}

impl TodoGroup {
    pub fn is_local(&self) -> bool {
        self.key.is_none()
    }

    pub fn open_count(&self) -> usize {
        self.items.iter().filter(|i| !i.done).count()
    }
}
