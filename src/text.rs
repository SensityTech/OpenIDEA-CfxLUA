use openidea_plugin::{Position, Range};

#[derive(Clone, Debug)]
// IDE positions are 0-based lines and UTF-16 columns; the analysis works on UTF-8 byte offsets.
pub struct LineIndex {
    starts: Vec<usize>,
    len: usize,
}

pub struct Cursor<'a> {
    index: &'a LineIndex,
    text: &'a str,
    line: usize,
    at: usize,
    character: u32,
}

impl Cursor<'_> {
    pub fn position(&mut self, at: usize) -> Position {
        let at = floor_char_boundary(self.text, at.min(self.text.len()));
        if at < self.at {
            self.line = self.index.line_of(at);
            self.at = self.index.starts[self.line];
            self.character = 0;
        }
        while self.index.starts.get(self.line + 1).is_some_and(|next| *next <= at) {
            self.line += 1;
            self.at = self.index.starts[self.line];
            self.character = 0;
        }
        let gap = &self.text[self.at..at];
        self.character +=
            if gap.is_ascii() { gap.len() as u32 } else { gap.chars().map(|c| c.len_utf16() as u32).sum() };
        self.at = at;
        Position::new(self.line as u32, self.character)
    }

    pub fn range(&mut self, start: usize, end: usize) -> Range {
        let start = self.position(start);
        Range::new(start, self.position(end))
    }
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Self { starts, len: text.len() }
    }

    pub fn line_of(&self, at: usize) -> usize {
        match self.starts.binary_search(&at.min(self.len)) {
            Ok(line) => line,
            Err(next) => next - 1,
        }
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.len)
    }

    pub fn line_end(&self, text: &str, line: usize) -> usize {
        let end = self.starts.get(line + 1).map_or(self.len, |s| s - 1);
        if end > 0 && text.as_bytes().get(end - 1) == Some(&b'\r') { end - 1 } else { end }
    }

    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    pub fn position(&self, text: &str, at: usize) -> Position {
        let at = floor_char_boundary(text, at.min(text.len()));
        let line = self.line_of(at);
        let start = self.starts[line];
        let character = text[start..at].chars().map(|c| c.len_utf16() as u32).sum();
        Position::new(line as u32, character)
    }

    pub fn range(&self, text: &str, start: usize, end: usize) -> Range {
        Range::new(self.position(text, start), self.position(text, end))
    }

    /// Positions for offsets asked in increasing order: each one is counted from the one before.
    pub fn cursor<'a>(&'a self, text: &'a str) -> Cursor<'a> {
        Cursor { index: self, text, line: 0, at: 0, character: 0 }
    }

    pub fn offset(&self, text: &str, pos: Position) -> usize {
        let line = pos.line as usize;
        if line >= self.starts.len() {
            return text.len();
        }
        let start = self.starts[line];
        let end = self.line_end(text, line);
        let mut units = 0u32;
        for (i, ch) in text[start..end].char_indices() {
            if units >= pos.character {
                return start + i;
            }
            units += ch.len_utf16() as u32;
        }
        end
    }
}

pub fn floor_char_boundary(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

pub fn edit_distance(a: &str, b: &str, max: usize) -> usize {
    let a: Vec<char> = a.chars().map(|c| c.to_ascii_lowercase()).collect();
    let b: Vec<char> = b.chars().map(|c| c.to_ascii_lowercase()).collect();
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(cur[j]);
        }
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn slash(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

pub fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_gives_the_positions_of_increasing_offsets() {
        let text = "ab\n\u{e9}t\u{e9} \u{1f600}x\r\nlast";
        let index = LineIndex::new(text);
        let mut cursor = index.cursor(text);
        for at in (0..=text.len()).filter(|at| text.is_char_boundary(*at)) {
            assert_eq!(cursor.position(at), index.position(text, at), "offset {at}");
        }
        assert_eq!(cursor.position(1), index.position(text, 1), "an offset before the last one starts over");
    }

    #[test]
    fn positions_round_trip_with_utf16() {
        let text = "ab\r\n😀x\nlast";
        let idx = LineIndex::new(text);
        for at in [0, 1, 2, 4, 8, 9, 10, text.len()] {
            assert_eq!(idx.offset(text, idx.position(text, at)), at, "{at}");
        }
        assert_eq!(idx.position(text, 8), Position::new(1, 2));
        assert_eq!(idx.offset(text, Position::new(0, 99)), 2, "clamped before \\r\\n");
        assert_eq!(idx.offset(text, Position::new(9, 0)), text.len());
    }

    #[test]
    fn distances() {
        assert_eq!(edit_distance("job", "jbo", 3), 2);
        assert_eq!(edit_distance("Job", "job", 3), 0);
        assert_eq!(edit_distance("weather", "wether", 2), 1);
        assert_eq!(edit_distance("a", "abcdef", 2), 3);
    }
}
