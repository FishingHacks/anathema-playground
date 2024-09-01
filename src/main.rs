use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use anathema::{
    backend::{tui::Style, Backend},
    component::*,
    default_widgets::Canvas,
    prelude::*,
    templates::blueprints::Blueprint,
    widgets::{components::events::KeyState, Element},
};
use temp_file::TempFile;

#[derive(State)]
struct EditorState {
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

struct Editor {
    lines: Vec<String>,
    offset_y: usize,
    cursor_x: usize,
    cursor_y: usize,
    file: File,
    // while this is >0, tick() repaints the canvas
    should_rerender: u8,
}

impl Editor {
    pub fn new(file: File) -> Self {
        let lines = std::fs::read_to_string(file.path())
            .expect("failed to read the specified path")
            .lines()
            .map(str::to_string)
            .collect();

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

            for x in 0..size.width {
                match line.next() {
                    Some(c) => canvas.put(c, Style::new(), (x as u16, y as u16)),
                    None => canvas.erase((x as u16, y as u16)),
                }
            }
        }

        if draw_cursor {
            let cursor_pos = (self.cursor_x as u16, self.cursor_y as u16);
            let char = canvas.get(cursor_pos).map(|v| v.0).unwrap_or(' ');
            let mut style = Style::new();
            style.set_inverse(true);
            canvas.put(char, style, cursor_pos);
        }
    }

    fn run_code(&mut self, mut context: Context<'_, EditorState>) {
        let mut string = String::with_capacity(self.lines.len() * 20);
        if self.lines.len() == 0 || (self.lines.len() == 1 && self.lines[0].len() == 0) {
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
                return;
            }
            Ok((blueprint, _)) => {
                if let Err(e) = validate_blueprint(&blueprint) {
                    ERROR.replace(format!("Failed to compile the template: {e}"));
                    context.publish("error", |state| &state.focused);
                    return;
                }
                match std::fs::write(self.file.path(), string.as_bytes()) {
                    Err(e) => {
                        ERROR.replace(format!("Failed to write the template: {e:?}"));
                        context.publish("error", |state| &state.focused);
                        return;
                    }
                    Ok(..) => {
                        context.publish("run", |state| &state.focused);
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

thread_local!(static ERROR: RefCell<String> = Default::default());

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
        if matches!(key.state, KeyState::Release) {
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

struct Playground(ComponentId<bool>);

#[derive(State)]
enum Showing {
    Editor,
    Error { errors: Value<List<String>> },
    Preview,
}

#[derive(State)]
struct PlaygroundState {
    showing: Value<Showing>,
}

impl Component for Playground {
    type Message = ();
    type State = PlaygroundState;

    fn receive(
        &mut self,
        ident: &str,
        _: CommonVal<'_>,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        if ident == "run_aml" {
            *state.showing.to_mut() = Showing::Preview;
        } else if ident == "editor_error" {
            *state.showing.to_mut() = Showing::Error {
                errors: ERROR
                    .with_borrow(|str| List::from_iter(str.split('\n').map(str::to_string))),
            };
        }
    }

    fn on_key(
        &mut self,
        key: KeyEvent,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        ctx: Context<'_, Self::State>,
    ) {
        if matches!(
            key,
            KeyEvent {
                code: KeyCode::Char('e'),
                ctrl: true,
                state: KeyState::Press
            }
        ) {
            *state.showing.to_mut() = Showing::Editor;
            ctx.emitter
                .emit(self.0, false)
                .expect("failed to notify the editor to redraw the canvas");
        }
    }
}

enum File {
    Temp(TempFile),
    Permanent(PathBuf),
}

impl File {
    pub fn path(&self) -> &Path {
        match self {
            Self::Temp(f) => f.path(),
            Self::Permanent(f) => f.as_path(),
        }
    }
}

macro_rules! release_bundle {
    ($path: expr) => {
        {
            #[cfg(debug_assertions)]
            { ($path as &'static str).to_path() }
            #[cfg(not(debug_assertions))]
            { include_str!(concat!("../", $path)).to_template() }
        }
    };
}

fn main() {
    let mut current_executable =
        std::env::current_exe().expect("Failed to get path to the current executable");
    if let Ok(path) = current_executable.strip_prefix(std::env::current_dir().unwrap_or_default()) {
        current_executable = path.to_path_buf();
    }
    
    let file = match std::env::args()
        .skip(1)
        .next()
    {
        Some(v) if v == "-h" || v == "--help" => {
            println!("Usage: {} [options] [path]\n", current_executable.display());
            println!("  -h --help: Display help information\n");
            println!("  uses a temporary file if no path was specified");
            return;
        }
        Some(path) => {
            let path = if !path.starts_with('/') {
                std::env::current_dir()
                    .expect("Failed to get the current directory")
                    .join(path)
            } else {
                PathBuf::from(path)
            };
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&path, "vstack\n").expect("failed to open the specified path");
            }
            File::Permanent(path)
        }
        _ => {
            let tempfile =
                TempFile::with_suffix(".aml").expect("failed to create a temporary file");
            std::fs::write(tempfile.path(), "vstack\n")
                .expect("failed to write to the temporary file");
            File::Temp(tempfile)
        }
    };

    let backend = TuiBackend::builder()
        .enable_alt_screen()
        .enable_raw_mode()
        .hide_cursor()
        .finish()
        .expect("failed to build the backend");

    let mut size = backend.size();
    size.width -= 2;
    size.height -= 2;

    let mut runtime = Runtime::builder(Document::new("@main"), backend);
    runtime
        .register_default::<()>("tempfile", SourceKind::Path(file.path().to_path_buf()))
        .unwrap();
    runtime
        .register_default::<()>("error", release_bundle!("templates/error.aml"))
        .unwrap();
    let editor = runtime
        .register_component(
            "editor",
            release_bundle!("templates/editor.aml"),
            Editor::new(file),
            size.into(),
        )
        .unwrap();
    runtime
        .register_component(
            "main",
            release_bundle!("templates/main.aml"),
            Playground(editor),
            PlaygroundState {
                showing: Showing::Editor.into(),
            },
        )
        .unwrap();

    let mut rt = runtime.finish().expect("failed to build the runtime");
    rt.fps = 60;
    rt.run();
}
