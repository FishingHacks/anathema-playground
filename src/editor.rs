use std::{cell::RefCell, path::PathBuf};

use anathema::{
    backend::tui::{Color, Style},
    component::{Component, KeyCode, KeyEvent},
    default_widgets::Canvas,
    prelude::{Context, Document},
    state::{State, Value},
    templates::blueprints::Blueprint,
    widgets::{components::events::KeyState, Element, Elements},
};

use crate::thread_backend::{launch_threaded_anathema, AnathemaThreadHandle};

#[derive(State)]
pub struct EditorState {
    width: Value<usize>,
    height: Value<usize>,
    focused: Value<bool>,
}
impl From<anathema::geometry::Size> for EditorState {
    fn from(value: anathema::geometry::Size) -> Self {
        Self {
            width: value.width.into(),
            height: value.height.into(),
            focused: false.into(),
        }
    }
}

pub struct Editor {
    lines: Vec<String>,
    offset_y: usize,
    cursor_x: usize,
    cursor_y: usize,
    file: Option<PathBuf>,
    // while this is >0, tick() repaints the canvas
    should_rerender: u8,
}

impl Editor {
    pub fn new(file: Option<PathBuf>) -> Self {
        let lines = match &file {
            Some(file) => std::fs::read_to_string(file)
                .expect("failed to read the specified path")
                .lines()
                .map(str::to_string)
                .collect::<Vec<String>>(),
            None => vec!["vstack".into(), "".into()],
        };

        Self {
            file,
            cursor_x: 0,
            cursor_y: 0,
            offset_y: 0,
            lines,
            should_rerender: 3,
        }
    }

    fn draw_canvas(&self, canvas: &mut Element<'_>, draw_cursor: bool) {
        let size = canvas.size();
        let Some(canvas) = canvas.try_to::<Canvas>() else {
            return;
        };

        for y in 0..size.height {
            let mut line = self
                .lines
                .get(y + self.offset_y)
                .map(String::as_str)
                .unwrap_or_default()
                .chars();

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
                    Some(c) => canvas.put(c, Style::new(), (x as u16, y as u16)),
                    None => canvas.erase((x as u16, y as u16)),
                }
            }
        }

        if draw_cursor {
            let cursor_pos = (self.cursor_x as u16 + 5, self.cursor_y as u16);
            let char = canvas.get(cursor_pos).map(|v| v.0).unwrap_or(' ');
            let mut style = Style::new();
            style.set_inverse(true);
            canvas.put(char, style, cursor_pos);
        }
    }

    fn run_code(&mut self, mut context: Context<'_, EditorState>) {
        if let Some(handle) = THREAD_HANDLE.take() {
            handle.close();
        }

        let mut string = String::with_capacity(self.lines.len() * 20);
        if self.lines.is_empty() || (self.lines.len() == 1 && self.lines[0].is_empty()) {
            string.push_str("text \"nothing to see here\"");
        } else {
            for line in self.lines.iter() {
                string.push_str(line);
                string.push('\n');
            }
        }

        self.should_rerender = 3;
        match Document::new(string.clone()).compile() {
            Err(e) => {
                ERROR.replace(format!("Failed to compile the template: {e:?}"));
                context.publish("error", |state| &state.focused);
            }
            Ok((blueprint, _)) => {
                if let Err(e) = validate_blueprint(&blueprint) {
                    ERROR.replace(format!("Failed to compile the template: {e}"));
                    context.publish("error", |state| &state.focused);
                    return;
                }
                if let Some(file) = self.file.as_ref() {
                    if let Err(e) = std::fs::write(file, string.as_bytes()) {
                        ERROR.replace(format!("Failed to write the template: {e:?}"));
                        context.publish("error", |state| &state.focused);
                        return;
                    }
                }
                match launch_threaded_anathema(string, context.viewport.size()) {
                    Err(e) => {
                        ERROR.replace(format!("Failed to write the template: {e:?}"));
                        context.publish("error", |state| &state.focused);
                    }
                    Ok(handle) => {
                        context.publish("run", |state| &state.focused);
                        THREAD_HANDLE.set(Some(handle));
                    }
                }
            }
        }
    }
}

static VALID_COMPONENTS: &[&str] = &[
    "text",
    "span",
    "border",
    "align",
    "vstack",
    "hstack",
    "zstack",
    "expand",
    "spacer",
    "position",
    "overflow",
    "canvas",
    "container",
];

fn validate_blueprint<'a>(blueprint: &'a Blueprint) -> Result<(), String> {
    let mut blueprints_to_validate: Vec<&'a Blueprint> = vec![blueprint];
    while blueprints_to_validate.len() > 0 {
        let Some(blueprint) = blueprints_to_validate.pop() else {
            break;
        };
        match blueprint {
            Blueprint::Component(component) => blueprints_to_validate.extend(component.body.iter()),
            Blueprint::ControlFlow(control_flow) => blueprints_to_validate.extend(
                control_flow.if_node.body.iter().chain(
                    control_flow
                        .elses
                        .iter()
                        .flat_map(|else_node| else_node.body.iter()),
                ),
            ),
            Blueprint::For(for_loop) => blueprints_to_validate.extend(for_loop.body.iter()),
            Blueprint::Single(widget) => {
                blueprints_to_validate.extend(widget.children.iter());
                if !VALID_COMPONENTS
                    .iter()
                    .any(|widget_name| *widget.ident == **widget_name)
                {
                    return Err(format!("Could not find widget `{}`", &*widget.ident));
                }
            }
        }
    }
    Ok(())
}

thread_local!(pub static ERROR: RefCell<String> = Default::default());
thread_local!(pub static THREAD_HANDLE: RefCell<Option<AnathemaThreadHandle>> = Default::default());

impl Component for Editor {
    // if true: run code
    // if false: redraw canvas
    type Message = bool;
    type State = EditorState;

    fn on_key(
        &mut self,
        key: KeyEvent,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        context: Context<'_, Self::State>,
    ) {
        if !*state.focused.to_ref() || matches!(key.state, KeyState::Release) {
            return;
        }
        let height = *state.height.to_ref();

        match key.code {
            KeyCode::Char('r') if key.ctrl => {
                *state.focused.to_mut() = false;
                self.should_rerender = 0;
                return self.run_code(context);
            }
            KeyCode::Char(' ') if key.ctrl => {
                if let Some(line) = self.lines.get_mut(self.offset_y + self.cursor_y) {
                    if self.cursor_x < line.len() {
                        line.insert_str(self.cursor_x, "    ");
                        self.cursor_x += 4;
                    } else {
                        line.push_str("    ");
                        self.cursor_x = line.len();
                    }
                } else {
                    self.lines.push(String::from("    "));
                    self.cursor_x = 4;
                    self.cursor_y += 1;
                    if self.cursor_y + self.offset_y >= self.lines.len() {
                        if self.offset_y >= self.lines.len() {
                            self.offset_y = self.lines.len().saturating_sub(1);
                        }
                        self.cursor_y = self.lines.len() - self.offset_y - 1;
                    }

                    if self.cursor_y + self.offset_y >= height {
                        self.offset_y = self.cursor_y + self.offset_y - height + 1;
                        self.cursor_y = height - 1;
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(line) = self.lines.get_mut(self.offset_y + self.cursor_y) {
                    if self.cursor_x < line.len() {
                        line.insert(self.cursor_x, c);
                        self.cursor_x += 1;
                    } else {
                        line.push(c);
                        self.cursor_x = line.len();
                    }
                } else {
                    self.lines.push(String::from(c));
                    self.cursor_x = 1;
                    self.cursor_y += 1;
                    if self.cursor_y + self.offset_y >= self.lines.len() {
                        if self.offset_y >= self.lines.len() {
                            self.offset_y = self.lines.len().saturating_sub(1);
                        }
                        self.cursor_y = self.lines.len() - self.offset_y - 1;
                    }

                    if self.cursor_y + self.offset_y >= height {
                        self.offset_y = self.cursor_y + self.offset_y - height + 1;
                        self.cursor_y = height - 1;
                    }
                }
            }
            KeyCode::Enter => {
                let new_line = if let Some(line) = self.lines.get_mut(self.cursor_y + self.offset_y)
                {
                    line.split_off(self.cursor_x)
                } else {
                    String::new()
                };
                self.cursor_x = 0;
                self.cursor_y += 1;

                if self.cursor_y >= self.lines.len() {
                    self.lines.push(new_line);
                    self.cursor_y = self.lines.len() - 1;
                    if self.cursor_y >= height {
                        self.offset_y = self.cursor_y - height + 1;
                        self.cursor_y = height - 1;
                    }
                } else {
                    self.lines.insert(self.offset_y + self.cursor_y, new_line);
                    if self.cursor_y + self.offset_y >= height {
                        self.offset_y = self.cursor_y + self.offset_y - height + 1;
                        self.cursor_y = height - 1;
                    }
                }
            }
            KeyCode::Backspace => {
                if self.cursor_y + self.offset_y == 0 && self.cursor_x == 0 {
                } else {
                    if let Some(line) = self.lines.get_mut(self.cursor_y + self.offset_y) {
                        if self.cursor_x == 0 {
                            if self.cursor_y + self.offset_y == 0 {
                                return;
                            }
                            let line = self.lines.remove(self.cursor_y + self.offset_y);
                            self.cursor_x = self.lines[self.cursor_y + self.offset_y - 1].len();
                            self.lines[self.cursor_y + self.offset_y - 1].extend(line.chars());

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

                        if self.cursor_y + self.offset_y >= height {
                            self.offset_y = self.cursor_y + self.offset_y - height + 1;
                            self.cursor_y = height - 1;
                        }
                        self.cursor_x = self
                            .lines
                            .get(self.cursor_y + self.offset_y)
                            .map(String::len)
                            .unwrap_or_default();
                    }
                }
            }
            KeyCode::Home if key.ctrl => {
                self.cursor_x = 0;
                self.cursor_y = 0;
                self.offset_y = 0;
            }
            KeyCode::Home => self.cursor_x = 0,
            KeyCode::End if key.ctrl => {
                self.cursor_y = self.lines.len().saturating_sub(1);
                self.cursor_x = self
                    .lines
                    .get(self.cursor_y)
                    .map(String::len)
                    .unwrap_or_default();
                if self.cursor_y >= height {
                    self.offset_y = self.cursor_y - height + 1;
                    self.cursor_y = height - 1;
                }
            }
            KeyCode::End => {
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
                    self.cursor_x = self.lines.get(self.cursor_y).map(String::len).unwrap_or(0);
                }
            }
            KeyCode::Down => {
                if self.cursor_y + self.offset_y + 1 < self.lines.len() {
                    self.cursor_y += 1;
                    if self.cursor_y + self.offset_y >= height {
                        self.offset_y = self.cursor_y + self.offset_y - height + 1;
                        self.cursor_y = height - 1;
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
                    self.cursor_x = self.lines.get(self.cursor_y).map(String::len).unwrap_or(0);
                }
            }
            KeyCode::Right => {
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
                        .map(String::len)
                        .unwrap_or_default()
                {
                    if self.offset_y + self.cursor_y + 1 >= self.lines.len() {
                        return;
                    }
                    self.cursor_x = 0;
                    if self.cursor_y + self.offset_y + 1 < self.lines.len() {
                        self.cursor_y += 1;
                        if self.cursor_y + self.offset_y >= height {
                            self.offset_y = self.cursor_y + self.offset_y - height + 1;
                            self.cursor_y = height - 1;
                        }
                    } else if self.lines.len() == 0 {
                        self.cursor_y = 0;
                        self.offset_y = 0;
                    } else {
                        if self.cursor_y + self.offset_y >= self.lines.len() {
                            if self.offset_y >= self.lines.len() {
                                self.offset_y = self.lines.len().saturating_sub(1);
                            }
                            self.cursor_y = self.lines.len() - self.offset_y;
                        }
                    }
                } else {
                    self.cursor_x += 1;
                }
            }

            KeyCode::Up => {
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

                    if let Some(len) = self
                        .lines
                        .get(self.offset_y + self.cursor_y)
                        .map(String::len)
                    {
                        self.cursor_x = self.cursor_x.min(len);
                    }
                }
            }
            KeyCode::Left => {
                if self.cursor_x == 0 {
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

                    if let Some(len) = self
                        .lines
                        .get(self.offset_y + self.cursor_y)
                        .map(String::len)
                    {
                        self.cursor_x = len;
                    }
                } else {
                    self.cursor_x -= 1;
                }
            }

            _ => return,
        }

        elements
            .by_tag("canvas")
            .first(|el, _| self.draw_canvas(el, true));
    }

    fn resize(
        &mut self,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        context: Context<'_, Self::State>,
    ) {
        let size = context.viewport.size();
        *state.height.to_mut() = size.height - 2;
        *state.width.to_mut() = size.width - 2;
        elements
            .by_tag("canvas")
            .first(|el, _| self.draw_canvas(el, *state.focused.to_ref()));
    }

    fn on_focus(
        &mut self,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        *state.focused.to_mut() = true;
        elements
            .by_tag("canvas")
            .first(|el, _| self.draw_canvas(el, true));
    }

    fn on_blur(
        &mut self,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        *state.focused.to_mut() = false;
        elements
            .by_tag("canvas")
            .first(|el, _| self.draw_canvas(el, false));
    }

    fn tick(
        &mut self,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
        _: std::time::Duration,
    ) {
        if self.should_rerender == 0 {
            return;
        }
        self.should_rerender -= 1;

        elements
            .by_tag("canvas")
            .first(|el, _| self.draw_canvas(el, *state.focused.to_ref()));
    }

    fn message(
        &mut self,
        _: Self::Message,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        self.should_rerender = 3;
        elements
            .by_tag("canvas")
            .first(|el, _| self.draw_canvas(el, *state.focused.to_ref()));
    }

    fn accept_focus(&self) -> bool {
        true
    }
}
