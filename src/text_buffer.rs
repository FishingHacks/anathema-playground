use anathema::{
    backend::tui::{Color, Style},
    default_widgets::Canvas,
    geometry::Size,
    widgets::Elements,
};

use crate::editor::VALID_WIDGETS;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum HighlightingStyle {
    None = 0,
    Number,
    String,
    HexVal,
    Component,
    Widget,
    Braces,
    Comment,
    Boolean,
}

impl HighlightingStyle {
    pub fn to_style(&self) -> Style {
        let mut style = Style::new();
        match self {
            Self::None => {}
            Self::Number => style.set_fg(Color::DarkYellow),
            Self::String => style.set_fg(Color::Green),
            Self::Braces => style.set_fg(Color::Blue),
            Self::Boolean => style.set_fg(Color::Magenta),
            Self::Component => style.set_fg(Color::DarkMagenta),
            Self::Widget => style.set_fg(Color::Cyan),
            Self::HexVal => style.set_fg(Color::DarkBlue),
            Self::Comment => {
                style.set_fg(Color::DarkGrey);
                style.set_italic(true);
            }
        }
        style
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Cell(char, HighlightingStyle);

impl From<char> for Cell {
    fn from(value: char) -> Self {
        Self(value, HighlightingStyle::None)
    }
}

pub struct TextBuffer {
    lines: Vec<Vec<Cell>>,
    offset_y: usize,
    cursor_x: usize,
    cursor_y: usize,
    width: usize,
    height: usize,
}

// editing
impl TextBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            lines: vec![vec![]],
            offset_y: 0,
            cursor_x: 0,
            cursor_y: 0,
            width,
            height,
        }
    }

    pub fn from_iter(mut iter: impl Iterator<Item = char>, width: usize, height: usize) -> Self {
        let mut lines = vec![vec![]];
        while let Some(c) = iter.next() {
            if c == '\n' {
                lines.push(vec![]);
            } else {
                lines
                    .last_mut()
                    .expect("the lines buffer should have at least 1 element")
                    .push(Cell(c, HighlightingStyle::None));
            }
        }

        Self {
            lines,
            cursor_x: 0,
            cursor_y: 0,
            offset_y: 0,
            width,
            height,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            return self.insert_newline();
        }

        if let Some(line) = self.lines.get_mut(self.offset_y + self.cursor_y) {
            if self.cursor_x < line.len() {
                line.insert(self.cursor_x, c.into());
                self.cursor_x += 1;
            } else {
                line.push(c.into());
                self.cursor_x = line.len();
            }
        } else {
            self.lines.push(vec![c.into()]);
            self.cursor_x = 1;
            self.cursor_y += 1;
            if self.cursor_y + self.offset_y >= self.lines.len() {
                if self.offset_y >= self.lines.len() {
                    self.offset_y = self.lines.len().saturating_sub(1);
                }
                self.cursor_y = self.lines.len() - self.offset_y - 1;
            }

            if self.cursor_y + self.offset_y >= self.height {
                self.offset_y = self.cursor_y + self.offset_y - self.height + 1;
                self.cursor_y = self.height - 1;
            }
        }
    }

    fn insert_newline(&mut self) {
        let new_line = if let Some(line) = self.lines.get_mut(self.cursor_y + self.offset_y) {
            line.split_off(self.cursor_x)
        } else {
            vec![]
        };
        self.cursor_x = 0;
        self.cursor_y += 1;

        if self.cursor_y >= self.lines.len() {
            self.lines.push(new_line);
            self.cursor_y = self.lines.len() - 1;
            if self.cursor_y >= self.height {
                self.offset_y = self.cursor_y - self.height + 1;
                self.cursor_y = self.height - 1;
            }
        } else {
            self.lines.insert(self.offset_y + self.cursor_y, new_line);
            if self.cursor_y + self.offset_y >= self.height {
                self.offset_y = self.cursor_y + self.offset_y - self.height + 1;
                self.cursor_y = self.height - 1;
            }
        }
    }

    pub fn remove_char_before(&mut self) {
        if self.cursor_y + self.offset_y == 0 && self.cursor_x == 0 {
        } else {
            if let Some(line) = self.lines.get_mut(self.cursor_y + self.offset_y) {
                if self.cursor_x == 0 {
                    if self.cursor_y + self.offset_y == 0 {
                        return;
                    }
                    let line = self.lines.remove(self.cursor_y + self.offset_y);
                    self.cursor_x = self.lines[self.cursor_y + self.offset_y - 1].len();
                    self.lines[self.cursor_y + self.offset_y - 1].extend(line.iter().copied());

                    if self.cursor_y == 0 {
                        self.offset_y -= 1;
                    } else {
                        self.cursor_y -= 1;
                    }
                } else if self.cursor_x >= line.len() {
                    line.pop();
                    self.cursor_x = line.len();
                } else {
                    line.remove(self.cursor_x - 1);
                    self.cursor_x -= 1;
                }
            } else {
                if self.cursor_y + self.offset_y >= self.lines.len() {
                    if self.offset_y >= self.lines.len() {
                        self.offset_y = self.lines.len().saturating_sub(1);
                    }
                    self.cursor_y = self.lines.len() - self.offset_y;
                }

                if self.cursor_y + self.offset_y >= self.height {
                    self.offset_y = self.cursor_y + self.offset_y - self.height + 1;
                    self.cursor_y = self.height - 1;
                }
                self.cursor_x = self
                    .lines
                    .get(self.cursor_y + self.offset_y)
                    .map(Vec::len)
                    .unwrap_or_default();
            }
        }
    }

    pub fn move_to_start(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.offset_y = 0;
    }

    pub fn move_to_linestart(&mut self) {
        self.cursor_x = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor_y = self.lines.len().saturating_sub(1);
        self.cursor_x = self
            .lines
            .get(self.cursor_y)
            .map(Vec::len)
            .unwrap_or_default();
        if self.cursor_y >= self.height {
            self.offset_y = self.cursor_y - self.height + 1;
            self.cursor_y = self.height - 1;
        }
    }

    pub fn move_to_lineend(&mut self) {
        if self.lines.len() == 0 {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.offset_y = 0;
        } else {
            if self.cursor_y + self.offset_y >= self.lines.len() {
                if self.offset_y >= self.lines.len() {
                    self.offset_y = self.lines.len().saturating_sub(1);
                }
                self.cursor_y = self.lines.len() - self.offset_y;
            }
            self.cursor_x = self.lines.get(self.cursor_y).map(Vec::len).unwrap_or(0);
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_y + self.offset_y + 1 < self.lines.len() {
            self.cursor_y += 1;
            if self.cursor_y + self.offset_y >= self.height {
                self.offset_y = self.cursor_y + self.offset_y - self.height + 1;
                self.cursor_y = self.height - 1;
            }
            if let Some(len) = self.lines.get(self.cursor_y + self.offset_y).map(Vec::len) {
                self.cursor_x = self.cursor_x.min(len);
            }
        } else if self.lines.len() == 0 {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.offset_y = 0;
        } else {
            if self.cursor_y + self.offset_y >= self.lines.len() {
                if self.offset_y >= self.lines.len() {
                    self.offset_y = self.lines.len().saturating_sub(1);
                }
                self.cursor_y = self.lines.len() - self.offset_y;
            }
            self.cursor_x = self.lines.get(self.cursor_y).map(Vec::len).unwrap_or(0);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_y + self.offset_y >= self.lines.len() {
            if self.offset_y >= self.lines.len() {
                self.offset_y = self.lines.len().saturating_sub(1);
            }
            self.cursor_y = self.lines.len() - self.offset_y;
        }
        if self.lines.len() == 0 {
            return;
        }
        if self.cursor_x
            == self
                .lines
                .get(self.offset_y + self.cursor_y)
                .map(Vec::len)
                .unwrap_or_default()
        {
            self.cursor_x = 0;
            self.move_down();
        } else {
            self.cursor_x += 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_y + self.offset_y == 0 {
            self.cursor_x = 0;
        } else {
            if self.cursor_y + self.offset_y > self.lines.len() {
                if self.offset_y >= self.lines.len() {
                    self.offset_y = self.lines.len().saturating_sub(1);
                }
                self.cursor_y = self.lines.len() - self.offset_y;
            }

            if self.cursor_y == 0 {
                if self.offset_y > 0 {
                    self.offset_y -= 1;
                }
            } else {
                self.cursor_y -= 1;
            }

            if let Some(len) = self.lines.get(self.offset_y + self.cursor_y).map(Vec::len) {
                self.cursor_x = self.cursor_x.min(len);
            }
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_y + self.offset_y == 0 && self.cursor_x == 0 {
        } else if self.cursor_x == 0 {
            if self.cursor_y + self.offset_y > self.lines.len() {
                if self.offset_y >= self.lines.len() {
                    self.offset_y = self.lines.len().saturating_sub(1);
                }
                self.cursor_y = self.lines.len() - self.offset_y;
            }

            if self.cursor_y == 0 {
                self.offset_y -= 1;
            } else {
                self.cursor_y -= 1;
            }

            if let Some(len) = self.lines.get(self.offset_y + self.cursor_y).map(Vec::len) {
                self.cursor_x = len;
            }
        } else {
            self.cursor_x -= 1;
        }
    }
}

// drawing
impl TextBuffer {
    pub fn to_string(&self) -> String {
        let mut string = String::with_capacity(self.lines.len() * 20);

        for line in self.lines.iter() {
            string.extend(line.iter().map(|v| v.0));
            string.push('\n');
        }
        string.pop();

        string
    }

    pub fn resize(&mut self, new_width: usize, new_height: usize) {
        self.width = new_width;
        self.height = new_height;
    }

    pub fn draw(&self, mut elements: Elements, draw_cursor: bool) {
        elements.by_tag("canvas").first(|el, _| {
            let size = el.size();
            if let Some(canvas) = el.try_to::<Canvas>() {
                self.draw_to_canvas(canvas, draw_cursor, size);
            }
        });
    }

    fn draw_to_canvas(&self, canvas: &mut Canvas, draw_cursor: bool, size: Size) {
        for y in 0..size.height {
            let mut line = self
                .lines
                .get(y + self.offset_y)
                .map(|v| v.iter())
                .unwrap_or_default();

            let line_num = (self.offset_y + y + 1).to_string();
            let line_num = format!("{}{} ", " ".repeat(4 - line_num.len()), line_num);

            let mut line_num_style = Style::new();
            line_num_style.set_fg((88, 88, 88).into());

            for x in 1..5 {
                canvas.put(
                    line_num.chars().nth(x).unwrap(),
                    line_num_style,
                    (x as u16, y as u16),
                );
            }

            // probably safe to assume we wont have to show line numbers with more than 3 digits
            for x in 5..size.width {
                match line.next() {
                    Some(c) => canvas.put(c.0, c.1.to_style(), (x as u16, y as u16)),
                    None => canvas.erase((x as u16, y as u16)),
                }
            }
        }

        if draw_cursor {
            let cursor_pos = (self.cursor_x as u16 + 5, self.cursor_y as u16);
            let (char, mut style) = canvas.get(cursor_pos).unwrap_or((' ', Style::new()));
            let fg = style.fg.unwrap_or(Color::Grey);
            style.set_bg(fg);
            style.set_fg(Color::Black);
            canvas.put(char, style, cursor_pos);
        }
    }
}

// highlighting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HighlightState {
    None,
    Comment,
    Ident,
    Component,
    Number,
    Hex,
    String,
}

impl TextBuffer {
    pub fn highlight_all(&mut self) {
        for i in 0..self.lines.len() {
            self.highlight_line(i);
        }
    }

    pub fn highlight_current_line(&mut self) {
        if self.cursor_y + self.offset_y < self.lines.len() {
            self.highlight_line(self.cursor_y + self.offset_y);
        }
    }

    fn highlight_line(&mut self, line_idx: usize) {
        let line = &mut self.lines[line_idx];
        line.iter_mut().for_each(|v| v.1 = HighlightingStyle::None);

        let mut cur = String::new();
        let mut highlight_state = HighlightState::None;
        let mut started_at = 0usize;
        let mut current = 0usize;
        let mut str_is_escape = false;
        let mut str_starting_quote = ' ';

        while current < line.len() {
            let char = line[current].0;

            if highlight_state != HighlightState::String {
                match char {
                    '[' | ']' | '{' | '}' | '(' | ')' => {
                        line[current].1 = HighlightingStyle::Braces
                    }
                    _ => (),
                }
            }

            match highlight_state {
                HighlightState::Comment => line[current].1 = HighlightingStyle::Comment,
                HighlightState::Ident if matches!(char, 'a'..='z' | 'A'..='Z' | '_' | '|' | '0'..='9') => {
                    cur.push(char)
                }
                HighlightState::Ident => {
                    if cur == "loop" {
                        line[started_at..current]
                            .iter_mut()
                            .for_each(|v| v.1 = HighlightingStyle::Number);
                    } else if VALID_WIDGETS.iter().any(|v| *v == cur) {
                        line[started_at..current]
                            .iter_mut()
                            .for_each(|v| v.1 = HighlightingStyle::Widget);
                    } else if cur == "true" || cur == "false" {
                        line[started_at..current]
                            .iter_mut()
                            .for_each(|v| v.1 = HighlightingStyle::Boolean);
                    }
                    cur.clear();
                    highlight_state = HighlightState::None;
                }
                HighlightState::Hex if matches!(char, 'a'..='f' | 'A'..='F' | '0'..='9') => (),
                HighlightState::Hex => {
                    line[started_at..current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::HexVal);
                    highlight_state = HighlightState::None;
                }
                HighlightState::None => (),
                HighlightState::Component if matches!(char, 'a'..='z' | 'A'..='Z' | '_' | '|' | '0'..='9') => {
                    ()
                }
                HighlightState::Component => {
                    line[started_at..current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::Component);
                    highlight_state = HighlightState::None;
                }
                HighlightState::Number if matches!(char, '0'..='9' | '.') => (),
                HighlightState::Number => {
                    line[started_at..current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::Number);
                    highlight_state = HighlightState::None;
                }
                HighlightState::String if str_is_escape => str_is_escape = false,
                HighlightState::String if char == '\\' => str_is_escape = true,
                HighlightState::String if char == str_starting_quote => {
                    line[started_at..=current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::String);
                    highlight_state = HighlightState::None;
                    current += 1;
                    continue;
                }
                HighlightState::String => (),
            }

            if highlight_state == HighlightState::None {
                match (char, line.get(current + 1).map(|v| v.0).unwrap_or('\0')) {
                    ('@', _) => {
                        cur.clear();
                        started_at = current;
                        highlight_state = HighlightState::Component;
                    }
                    ('/', '/') => {
                        started_at = current;
                        line[current].1 = HighlightingStyle::Comment;
                        highlight_state = HighlightState::Comment;
                    }
                    ('0'..='9' | '.', _) | ('-', '0'..='9' | '.') => {
                        started_at = current;
                        highlight_state = HighlightState::Number;
                    }
                    ('\'' | '"', _) => {
                        started_at = current;
                        highlight_state = HighlightState::String;
                        str_starting_quote = char;
                    }
                    ('#', _) => {
                        started_at = current;
                        highlight_state = HighlightState::Hex;
                    }
                    ('a'..='z' | 'A'..='Z', _) => {
                        started_at = current;
                        highlight_state = HighlightState::Ident;
                        cur.clear();
                        cur.push(char);
                    }
                    _ => (),
                }
            }

            current += 1;
        }

        match highlight_state {
            HighlightState::Ident => {
                if cur == "loop" {
                    line[started_at..current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::Number);
                } else if VALID_WIDGETS.iter().any(|v| *v == cur) {
                    line[started_at..current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::Widget);
                } else if cur == "true" || cur == "false" {
                    line[started_at..current]
                        .iter_mut()
                        .for_each(|v| v.1 = HighlightingStyle::Boolean);
                }
            }
            HighlightState::Hex => line[started_at..current]
                .iter_mut()
                .for_each(|v| v.1 = HighlightingStyle::HexVal),
            HighlightState::Component => line[started_at..current]
                .iter_mut()
                .for_each(|v| v.1 = HighlightingStyle::Component),
            HighlightState::Number => line[started_at..current]
                .iter_mut()
                .for_each(|v| v.1 = HighlightingStyle::Number),
            HighlightState::Comment | HighlightState::None | HighlightState::String => (),
        }
    }
}
