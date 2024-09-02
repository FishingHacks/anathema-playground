use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use anathema::{
    component::{Component, KeyCode, KeyEvent},
    geometry::Size,
    prelude::{Context, Document},
    state::{State, Value},
    templates::blueprints::Blueprint,
    widgets::{components::events::KeyState, Elements},
};

use crate::{
    text_buffer::TextBuffer,
    thread_backend::{launch_threaded_anathema, AnathemaThreadHandle},
};

#[derive(State)]
pub struct EditorState {
    width: Value<usize>,
    height: Value<usize>,
    focused: Value<bool>,
    file: Value<String>,
}
impl EditorState {
    pub fn new(size: Size, file: Option<&Path>) -> Self {
        let filename = file
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("template.aml");

        Self {
            width: size.width.into(),
            height: size.height.into(),
            focused: false.into(),
            file: filename.to_string().into(),
        }
    }
}

pub struct Editor {
    buffer: TextBuffer,
    file: Option<PathBuf>,
    // while this is >0, tick() repaints the canvas
    should_rerender: u8,
}

impl Editor {
    pub fn new(file: Option<PathBuf>, size: Size) -> Self {
        let lines = match &file {
            Some(file) => {
                &std::fs::read_to_string(file).expect("failed to read the specified path")
            }
            None => "vstack\n",
        };
        let mut buffer = TextBuffer::from_iter(lines.chars(), size.width, size.height);
        buffer.highlight_all();

        Self {
            file,
            buffer,
            should_rerender: 3,
        }
    }

    fn run_code(&mut self, mut context: Context<'_, EditorState>) {
        if let Some(handle) = THREAD_HANDLE.take() {
            handle.close();
        }

        let string = self.buffer.to_string();

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

pub static VALID_WIDGETS: &[&str] = &[
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
                if !VALID_WIDGETS
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
    type Message = ();
    type State = EditorState;

    fn on_key(
        &mut self,
        key: KeyEvent,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        context: Context<'_, Self::State>,
    ) {
        if !*state.focused.to_ref() || matches!(key.state, KeyState::Release) {
            return;
        }

        match key.code {
            KeyCode::Char('r') if key.ctrl => {
                *state.focused.to_mut() = false;
                self.should_rerender = 0;
                return self.run_code(context);
            }
            KeyCode::Char(' ') if key.ctrl => {
                self.buffer.insert_char(' ');
                self.buffer.insert_char(' ');
                self.buffer.insert_char(' ');
                self.buffer.insert_char(' ');
                self.buffer.highlight_current_line();
            }
            KeyCode::Char(c) => {
                self.buffer.insert_char(c);
                self.buffer.highlight_current_line();
            }
            KeyCode::Enter => {
                self.buffer.insert_char('\n');
                self.buffer.highlight_current_line()
            }
            KeyCode::Backspace => {
                self.buffer.remove_char_before();
                self.buffer.highlight_current_line();
            }
            KeyCode::Home if key.ctrl => self.buffer.move_to_start(),
            KeyCode::End if key.ctrl => self.buffer.move_to_end(),
            KeyCode::Home => self.buffer.move_to_linestart(),
            KeyCode::End => self.buffer.move_to_lineend(),
            KeyCode::Down => self.buffer.move_down(),
            KeyCode::Right => self.buffer.move_right(),
            KeyCode::Up => self.buffer.move_up(),
            KeyCode::Left => self.buffer.move_left(),

            _ => return,
        }
    }

    fn resize(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        context: Context<'_, Self::State>,
    ) {
        let size = context.viewport.size();
        *state.height.to_mut() = size.height - 2;
        *state.width.to_mut() = size.width - 2;
        self.buffer.resize(size.width, size.height);
    }

    fn on_focus(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        *state.focused.to_mut() = true;
    }

    fn on_blur(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        *state.focused.to_mut() = false;
    }

    fn tick(
        &mut self,
        state: &mut Self::State,
        elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
        _: std::time::Duration,
    ) {
        self.buffer.draw(elements, *state.focused.to_ref());
    }

    fn accept_focus(&self) -> bool {
        true
    }
}
