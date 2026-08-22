//! A small modal text editor: vim motions and operators over one buffer.
//!
//! Used for checklist item text and for the ticket pane's comment box, so the
//! editing feel is the same wherever you type in tk. It is deliberately not a
//! vim clone — it covers the operations you reach for on a line of text and
//! stops there.
//!
//! Everything is indexed in *characters*, not bytes. The comment box used to
//! `String::pop()`, which chews a multi-byte character into fragments; here the
//! cursor is a char index and all slicing goes through `char_indices`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditMode {
    Normal,
    Insert,
}

/// What the host view should do after a keypress.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Still editing.
    Continue,
    /// `ZZ` / `Enter` in normal mode — keep the buffer.
    Commit,
    /// `Esc` in normal mode — throw the edit away.
    Cancel,
}

#[derive(Clone, Debug)]
pub struct Editor {
    chars: Vec<char>,
    /// Char index in 0..=len. In normal mode it rests *on* a character.
    cursor: usize,
    mode: EditMode,
    /// A half-typed operator or find (`d`, `c`, `g`, `f`, `t`, `Z`).
    pending: Option<char>,
    register: String,
    undo: Vec<(Vec<char>, usize)>,
    /// True while the buffer is a brand-new item, so cancelling drops it.
    pub fresh: bool,
}

/// Character classes for `w`/`b`/`e`, matching vim: word chars, punctuation,
/// whitespace.
fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

impl Editor {
    pub fn new(text: &str, mode: EditMode) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let cursor = if mode == EditMode::Insert {
            chars.len()
        } else {
            0
        };
        Self {
            chars,
            cursor,
            mode,
            pending: None,
            register: String::new(),
            undo: Vec::new(),
            fresh: false,
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn mode(&self) -> EditMode {
        self.mode
    }

    /// Cursor position in characters — what the renderer needs to place the
    /// caret, and what a test asserts against.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.chars.iter().all(|c| c.is_whitespace())
    }

    fn snapshot(&mut self) {
        self.undo.push((self.chars.clone(), self.cursor));
        // one line of text doesn't need infinite history
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
    }

    fn last(&self) -> usize {
        self.chars.len().saturating_sub(1)
    }

    /// In normal mode the cursor sits on a character, so it can't rest past the
    /// final one; in insert mode it may sit just after it.
    fn clamp(&mut self) {
        let max = match self.mode {
            EditMode::Normal => self.last(),
            EditMode::Insert => self.chars.len(),
        };
        self.cursor = self.cursor.min(max);
    }

    // ------------------------------------------------------------ motions --

    fn word_fwd(&self, mut i: usize) -> usize {
        let n = self.chars.len();
        if i >= n {
            return n;
        }
        let start = class(self.chars[i]);
        if start != 0 {
            while i < n && class(self.chars[i]) == start {
                i += 1;
            }
        }
        while i < n && class(self.chars[i]) == 0 {
            i += 1;
        }
        i
    }

    fn word_back(&self, mut i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        i -= 1;
        while i > 0 && class(self.chars[i]) == 0 {
            i -= 1;
        }
        let k = class(self.chars[i]);
        while i > 0 && class(self.chars[i - 1]) == k {
            i -= 1;
        }
        i
    }

    fn word_end(&self, mut i: usize) -> usize {
        let n = self.chars.len();
        if n == 0 {
            return 0;
        }
        i = (i + 1).min(n);
        while i < n && class(self.chars[i]) == 0 {
            i += 1;
        }
        if i >= n {
            return self.last();
        }
        let k = class(self.chars[i]);
        while i + 1 < n && class(self.chars[i + 1]) == k {
            i += 1;
        }
        i
    }

    fn first_non_blank(&self) -> usize {
        self.chars
            .iter()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0)
    }

    fn find_char(&self, target: char, till: bool) -> Option<usize> {
        let hit = self.chars[(self.cursor + 1).min(self.chars.len())..]
            .iter()
            .position(|&c| c == target)
            .map(|off| self.cursor + 1 + off)?;
        Some(if till { hit.saturating_sub(1) } else { hit })
    }

    // ----------------------------------------------------------- edits -----

    fn delete_range(&mut self, from: usize, to: usize) {
        let (from, to) = (from.min(to), to.max(from).min(self.chars.len()));
        if from >= to {
            return;
        }
        self.snapshot();
        self.register = self.chars[from..to].iter().collect();
        self.chars.drain(from..to);
        self.cursor = from;
        self.clamp();
    }

    fn insert_char(&mut self, c: char) {
        let at = self.cursor.min(self.chars.len());
        self.chars.insert(at, c);
        self.cursor = at + 1;
    }

    fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some((chars, cursor)) => {
                self.chars = chars;
                self.cursor = cursor;
                self.clamp();
                true
            }
            None => false,
        }
    }

    /// Resolve a motion key to a target index, for use as an operator's range.
    fn motion_target(&self, code: KeyCode, pending_find: Option<(char, bool)>) -> Option<usize> {
        if let Some((c, till)) = pending_find {
            return self.find_char(c, till).map(|i| i + 1);
        }
        Some(match code {
            KeyCode::Char('h') | KeyCode::Left => self.cursor.saturating_sub(1),
            KeyCode::Char('l') | KeyCode::Right => (self.cursor + 1).min(self.chars.len()),
            KeyCode::Char('w') => self.word_fwd(self.cursor),
            KeyCode::Char('b') => self.word_back(self.cursor),
            KeyCode::Char('e') => (self.word_end(self.cursor) + 1).min(self.chars.len()),
            KeyCode::Char('0') | KeyCode::Home => 0,
            KeyCode::Char('^') => self.first_non_blank(),
            KeyCode::Char('$') | KeyCode::End => self.chars.len(),
            _ => return None,
        })
    }

    pub fn key(&mut self, k: KeyEvent) -> Outcome {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match self.mode {
            EditMode::Insert => self.key_insert(k, ctrl),
            EditMode::Normal => self.key_normal(k, ctrl),
        }
    }

    fn key_insert(&mut self, k: KeyEvent, ctrl: bool) -> Outcome {
        match (k.code, ctrl) {
            (KeyCode::Esc, _) => {
                self.mode = EditMode::Normal;
                // vim leaves the cursor on the character you just passed
                self.cursor = self.cursor.saturating_sub(1);
                self.clamp();
            }
            (KeyCode::Enter, _) => return Outcome::Commit,
            (KeyCode::Backspace, _) => {
                if self.cursor > 0 {
                    self.snapshot();
                    self.cursor -= 1;
                    self.chars.remove(self.cursor);
                }
            }
            (KeyCode::Delete, _) => {
                if self.cursor < self.chars.len() {
                    self.snapshot();
                    self.chars.remove(self.cursor);
                }
            }
            (KeyCode::Left, _) => self.cursor = self.cursor.saturating_sub(1),
            (KeyCode::Right, _) => self.cursor = (self.cursor + 1).min(self.chars.len()),
            (KeyCode::Home, _) => self.cursor = 0,
            (KeyCode::End, _) => self.cursor = self.chars.len(),
            // ctrl-w: rub out the word before the cursor, as in a shell
            (KeyCode::Char('w'), true) => {
                let to = self.cursor;
                let from = self.word_back(self.cursor);
                self.delete_range(from, to);
            }
            (KeyCode::Char('u'), true) => {
                let to = self.cursor;
                self.delete_range(0, to);
            }
            (KeyCode::Char(c), false) => {
                if self.undo.is_empty() {
                    self.snapshot();
                }
                self.insert_char(c);
            }
            _ => {}
        }
        Outcome::Continue
    }

    fn key_normal(&mut self, k: KeyEvent, ctrl: bool) -> Outcome {
        let pending = self.pending.take();

        // a pending f/t consumes the next key as its target
        if let Some(p @ ('f' | 't')) = pending {
            if let KeyCode::Char(c) = k.code {
                if let Some(i) = self.find_char(c, p == 't') {
                    self.cursor = i;
                }
            }
            return Outcome::Continue;
        }
        // a pending operator consumes the next key as its motion
        if let Some(op @ ('d' | 'c')) = pending {
            // dd / cc — the whole line
            if matches!(k.code, KeyCode::Char(c) if c == op) {
                self.snapshot();
                self.register = self.text();
                self.chars.clear();
                self.cursor = 0;
                if op == 'c' {
                    self.mode = EditMode::Insert;
                }
                return Outcome::Continue;
            }
            if matches!(k.code, KeyCode::Char('f' | 't')) {
                // dfx / dtx would need one more key; not worth the state.
                return Outcome::Continue;
            }
            if let Some(target) = self.motion_target(k.code, None) {
                self.delete_range(self.cursor, target);
                if op == 'c' {
                    self.mode = EditMode::Insert;
                }
            }
            return Outcome::Continue;
        }
        if pending == Some('Z') {
            return match k.code {
                KeyCode::Char('Z') => Outcome::Commit,
                KeyCode::Char('Q') => Outcome::Cancel,
                _ => Outcome::Continue,
            };
        }

        match (k.code, ctrl) {
            (KeyCode::Esc, _) => return Outcome::Cancel,
            (KeyCode::Char('c'), true) => return Outcome::Cancel,
            (KeyCode::Enter, _) => return Outcome::Commit,
            (KeyCode::Char('Z'), _) => self.pending = Some('Z'),

            // motions
            (KeyCode::Char('h'), false) | (KeyCode::Left, _) => {
                self.cursor = self.cursor.saturating_sub(1)
            }
            (KeyCode::Char('l'), false) | (KeyCode::Right, _) => {
                self.cursor = (self.cursor + 1).min(self.last())
            }
            (KeyCode::Char('w'), false) => {
                self.cursor = self.word_fwd(self.cursor).min(self.last())
            }
            (KeyCode::Char('b'), false) => self.cursor = self.word_back(self.cursor),
            (KeyCode::Char('e'), false) => self.cursor = self.word_end(self.cursor),
            (KeyCode::Char('0'), _) | (KeyCode::Home, _) => self.cursor = 0,
            (KeyCode::Char('^'), _) => self.cursor = self.first_non_blank(),
            (KeyCode::Char('$'), _) | (KeyCode::End, _) => self.cursor = self.last(),
            (KeyCode::Char(p @ ('f' | 't')), false) => self.pending = Some(p),

            // entering insert
            (KeyCode::Char('i'), false) => self.mode = EditMode::Insert,
            (KeyCode::Char('a'), false) => {
                self.mode = EditMode::Insert;
                self.cursor = (self.cursor + 1).min(self.chars.len());
            }
            (KeyCode::Char('I'), _) => {
                self.mode = EditMode::Insert;
                self.cursor = self.first_non_blank();
            }
            (KeyCode::Char('A'), _) => {
                self.mode = EditMode::Insert;
                self.cursor = self.chars.len();
            }

            // operators
            (KeyCode::Char(op @ ('d' | 'c')), false) => self.pending = Some(op),
            (KeyCode::Char('x'), false) | (KeyCode::Delete, _) => {
                let to = (self.cursor + 1).min(self.chars.len());
                self.delete_range(self.cursor, to);
            }
            (KeyCode::Char('D'), _) => {
                let (from, to) = (self.cursor, self.chars.len());
                self.delete_range(from, to);
            }
            (KeyCode::Char('C'), _) => {
                let (from, to) = (self.cursor, self.chars.len());
                self.delete_range(from, to);
                self.mode = EditMode::Insert;
                self.cursor = self.chars.len();
            }
            (KeyCode::Char('s'), false) => {
                let to = (self.cursor + 1).min(self.chars.len());
                self.delete_range(self.cursor, to);
                self.mode = EditMode::Insert;
            }
            (KeyCode::Char('S'), _) => {
                self.snapshot();
                self.chars.clear();
                self.cursor = 0;
                self.mode = EditMode::Insert;
            }
            (KeyCode::Char('p'), false) => {
                let reg = self.register.clone();
                if !reg.is_empty() {
                    self.snapshot();
                    let at = (self.cursor + 1).min(self.chars.len());
                    for (i, c) in reg.chars().enumerate() {
                        self.chars.insert(at + i, c);
                    }
                    self.cursor = at + reg.chars().count() - 1;
                }
            }
            (KeyCode::Char('u'), false) => {
                self.undo();
            }
            _ => {}
        }
        self.clamp();
        Outcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str) -> Editor {
        Editor::new(text, EditMode::Normal)
    }

    fn press(e: &mut Editor, keys: &str) -> Outcome {
        let mut last = Outcome::Continue;
        for c in keys.chars() {
            let code = match c {
                '␛' => KeyCode::Esc,
                '⏎' => KeyCode::Enter,
                '⌫' => KeyCode::Backspace,
                c => KeyCode::Char(c),
            };
            last = e.key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        last
    }

    #[test]
    fn motions_land_where_vim_lands() {
        let cases = [
            ("w", 6),   // hello| world
            ("ww", 12), // to the end word
            ("$", 16),
            ("0", 0),
            ("e", 4),
            ("we", 10),
            ("$b", 12),
        ];
        for (keys, want) in cases {
            let mut e = ed("hello world again");
            press(&mut e, keys);
            assert_eq!(e.cursor(), want, "after {keys:?}");
        }
    }

    #[test]
    fn f_and_t_find_forward() {
        let mut e = ed("hello world");
        press(&mut e, "fw");
        assert_eq!(e.cursor(), 6);
        let mut e = ed("hello world");
        press(&mut e, "tw");
        assert_eq!(e.cursor(), 5);
        // a target that isn't there leaves the cursor alone
        let mut e = ed("hello");
        press(&mut e, "fz");
        assert_eq!(e.cursor(), 0);
    }

    #[test]
    fn operators_take_a_motion() {
        let mut e = ed("hello world again");
        press(&mut e, "dw");
        assert_eq!(e.text(), "world again");

        let mut e = ed("hello world");
        press(&mut e, "$d0");
        assert_eq!(e.text(), "d");

        let mut e = ed("hello world");
        press(&mut e, "wD");
        assert_eq!(e.text(), "hello ");
    }

    #[test]
    fn cw_deletes_the_word_and_enters_insert() {
        let mut e = ed("hello world");
        press(&mut e, "cw");
        assert_eq!(e.mode(), EditMode::Insert);
        press(&mut e, "goodbye ");
        assert_eq!(e.text(), "goodbye world");
    }

    #[test]
    fn dd_and_cc_take_the_whole_line() {
        let mut e = ed("throw me away");
        press(&mut e, "dd");
        assert_eq!(e.text(), "");
        assert_eq!(e.mode(), EditMode::Normal);

        let mut e = ed("replace me");
        press(&mut e, "cc");
        assert_eq!(e.text(), "");
        assert_eq!(e.mode(), EditMode::Insert);
        press(&mut e, "fresh");
        assert_eq!(e.text(), "fresh");
    }

    #[test]
    fn x_s_and_p_behave() {
        let mut e = ed("abcd");
        press(&mut e, "x");
        assert_eq!(e.text(), "bcd");
        press(&mut e, "p");
        assert_eq!(e.text(), "bacd");

        let mut e = ed("abcd");
        press(&mut e, "sZ");
        assert_eq!(e.text(), "Zbcd");
    }

    #[test]
    fn insert_entry_points_land_correctly() {
        let mut e = ed("hello");
        press(&mut e, "A!");
        assert_eq!(e.text(), "hello!");

        let mut e = ed("hello");
        press(&mut e, "I>");
        assert_eq!(e.text(), ">hello");

        let mut e = ed("hello");
        press(&mut e, "aX");
        assert_eq!(e.text(), "hXello");

        let mut e = ed("  indented");
        press(&mut e, "I>");
        assert_eq!(e.text(), "  >indented", "I goes to the first non-blank");
    }

    #[test]
    fn undo_restores_the_previous_buffer() {
        let mut e = ed("hello world");
        press(&mut e, "dw");
        assert_eq!(e.text(), "world");
        press(&mut e, "u");
        assert_eq!(e.text(), "hello world");
    }

    /// The bug this replaces: the old compose box popped bytes, which splits a
    /// multi-byte character into rubbish.
    #[test]
    fn multi_byte_characters_survive_editing() {
        let mut e = Editor::new("héllo — wörld", EditMode::Insert);
        e.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text(), "héllo — wörl");

        let mut e = ed("héllo — wörld");
        press(&mut e, "x");
        assert_eq!(e.text(), "éllo — wörld");
        press(&mut e, "dw");
        assert_eq!(e.text(), "— wörld");
    }

    #[test]
    fn commit_and_cancel_are_distinguishable() {
        let mut e = ed("text");
        assert_eq!(press(&mut e, "⏎"), Outcome::Commit);
        let mut e = ed("text");
        assert_eq!(press(&mut e, "ZZ"), Outcome::Commit);
        let mut e = ed("text");
        assert_eq!(press(&mut e, "␛"), Outcome::Cancel);
        // esc from insert only leaves insert; it doesn't cancel
        let mut e = Editor::new("text", EditMode::Insert);
        assert_eq!(press(&mut e, "␛"), Outcome::Continue);
        assert_eq!(e.mode(), EditMode::Normal);
    }

    #[test]
    fn the_normal_mode_cursor_cannot_sit_past_the_last_character() {
        let mut e = ed("abc");
        press(&mut e, "llllll");
        assert_eq!(e.cursor(), 2);
        press(&mut e, "A");
        assert_eq!(e.cursor(), 3, "but insert mode may sit after it");
    }

    #[test]
    fn an_empty_buffer_does_not_panic() {
        let mut e = ed("");
        press(&mut e, "xwbe$0dwdd u p");
        assert_eq!(e.text(), "");
    }
}
