use unicode_width::UnicodeWidthChar;

/// A multi-line text buffer with a cursor, edited one keystroke at a time.
#[derive(Debug, Default, Clone)]
pub struct Input {
    text: String,
    /// Byte offset of the cursor, always on a char boundary.
    cursor: usize,
}

impl Input {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if let Some(c) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= c.len_utf8();
            self.text.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        if let Some(c) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= c.len_utf8();
        }
    }

    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }

    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
    }

    pub fn end(&mut self) {
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i);
    }

    pub fn delete_word(&mut self) {
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end();
        let start = trimmed.rfind(char::is_whitespace).map_or(0, |i| {
            i + trimmed[i..].chars().next().map_or(1, char::len_utf8)
        });
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Splits the text into rows of at most `width` columns, breaking
    /// anywhere (not on words), and returns the cursor's (row, column).
    pub fn layout(&self, width: u16) -> (Vec<String>, (u16, u16)) {
        let width = width.max(1) as usize;
        let mut rows = vec![String::new()];
        let mut row_width = 0;
        let mut cursor = (0, 0);

        for (offset, c) in self.text.char_indices() {
            if offset == self.cursor {
                cursor = (rows.len() - 1, row_width);
            }
            if c == '\n' {
                rows.push(String::new());
                row_width = 0;
                continue;
            }
            let c_width = c.width().unwrap_or(0);
            if row_width + c_width > width {
                rows.push(String::new());
                row_width = 0;
            }
            rows.last_mut().unwrap().push(c);
            row_width += c_width;
        }
        if self.cursor == self.text.len() {
            cursor = (rows.len() - 1, row_width);
        }
        if cursor.1 >= width {
            rows.push(String::new());
            cursor = (rows.len() - 1, 0);
        }
        (rows, (cursor.0 as u16, cursor.1 as u16))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Input {
        let mut input = Input::default();
        input.insert_str(s);
        input
    }

    #[test]
    fn edits_around_the_cursor() {
        let mut input = typed("héllo");
        input.left();
        input.left();
        input.backspace();
        input.insert('L');
        assert_eq!(input.text(), "héLlo");
        input.delete();
        assert_eq!(input.text(), "héLo");
    }

    #[test]
    fn home_and_end_stay_on_the_current_line() {
        let mut input = typed("ab\ncd");
        input.home();
        input.insert('>');
        assert_eq!(input.text(), "ab\n>cd");
        input.end();
        input.insert('<');
        assert_eq!(input.text(), "ab\n>cd<");
    }

    #[test]
    fn deletes_the_previous_word() {
        let mut input = typed("salut tout le monde  ");
        input.delete_word();
        assert_eq!(input.text(), "salut tout le ");
    }

    #[test]
    fn wraps_and_places_the_cursor() {
        let input = typed("abcdef\ng");
        let (rows, cursor) = input.layout(4);
        assert_eq!(rows, ["abcd", "ef", "g"]);
        assert_eq!(cursor, (2, 1));
    }

    #[test]
    fn take_empties_the_buffer() {
        let mut input = typed("hi");
        assert_eq!(input.take(), "hi");
        assert!(input.is_empty());
        input.insert('x');
        assert_eq!(input.text(), "x");
    }
}
