use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use anathema::{
    component::{Component, KeyCode, KeyEvent},
    geometry::Size,
    prelude::Context,
    state::{State, Value},
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
    dirty: Value<bool>,
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
            dirty: false.into(),
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

    fn check_code(&mut self, mut context: Context<'_, EditorState>, dirty: &mut Value<bool>) {
        if let Some(handle) = THREAD_HANDLE.take() {
            handle.close();
        }

        let string = self.buffer.to_string();

        self.should_rerender = 3;
        dirty.set(false);
        match launch_threaded_anathema(string, context.viewport.size()) {
            Err(_) => (),
            Ok(handle) => {
                context.publish("run", |state| &state.focused);
                THREAD_HANDLE.set(Some(handle));
            }
        }
    }
}

thread_local!(pub static THREAD_HANDLE: RefCell<Option<AnathemaThreadHandle>> = Default::default());

impl Component for Editor {
    type Message = ();
    type State = EditorState;

    fn message(
        &mut self,
        _: Self::Message,
        _: &mut Self::State,
        _: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        self.should_rerender = 3;
    }

    fn on_key(
        &mut self,
        key: KeyEvent,
        state: &mut Self::State,
        elements: Elements<'_, '_>,
        context: Context<'_, Self::State>,
    ) {
        if !*state.focused.to_ref() || matches!(key.state, KeyState::Release) {
            return;
        }

        match key.code {
            KeyCode::Char('r') if key.ctrl => {
                state.focused.set(false);
                self.should_rerender = 0;
                return self.check_code(context, &mut state.dirty);
            }
            KeyCode::Char('s') if key.ctrl => {
                if let None = self
                    .file
                    .as_ref()
                    .and_then(|path| std::fs::write(path, self.buffer.to_string().as_bytes()).err())
                {
                    state.dirty.set(false);
                }
            }
            KeyCode::Char(' ') if key.ctrl => {
                state.dirty.set(true);
                self.buffer.insert_char(' ');
                self.buffer.insert_char(' ');
                self.buffer.insert_char(' ');
                self.buffer.insert_char(' ');
                self.buffer.highlight_current_line();
            }
            KeyCode::Char(c) => {
                state.dirty.set(true);
                self.buffer.insert_char(c);
                self.buffer.highlight_current_line();
            }
            KeyCode::Enter => {
                state.dirty.set(true);
                self.buffer.insert_char('\n');
                self.buffer.highlight_current_line()
            }
            KeyCode::Delete => {
                state.dirty.set(true);
                self.buffer.remove_char_after();
                self.buffer.highlight_current_line();
            }
            KeyCode::Backspace => {
                state.dirty.set(true);
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
            KeyCode::PageDown => {
                self.buffer.move_down();
                self.buffer.move_down();
                self.buffer.move_down();
                self.buffer.move_down();
                self.buffer.move_down();
            }
            KeyCode::PageUp => {
                self.buffer.move_up();
                self.buffer.move_up();
                self.buffer.move_up();
                self.buffer.move_up();
                self.buffer.move_up();
            }

            _ => return,
        }

        self.buffer.draw(elements, *state.focused.to_ref());
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
        self.should_rerender = 3;
    }

    fn on_focus(
        &mut self,
        state: &mut Self::State,
        elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        state.focused.set(true);
        self.buffer.draw(elements, *state.focused.to_ref());
    }

    fn on_blur(
        &mut self,
        state: &mut Self::State,
        elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        state.focused.set(false);
        self.buffer.draw(elements, *state.focused.to_ref());
    }

    fn tick(
        &mut self,
        state: &mut Self::State,
        elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
        _: std::time::Duration,
    ) {
        if self.should_rerender > 0 {
            self.buffer.draw(elements, *state.focused.to_ref());
            self.should_rerender -= 1;
        }
    }

    fn receive(
        &mut self,
        ident: &str,
        value: anathema::state::CommonVal<'_>,
        _: &mut Self::State,
        _: Elements<'_, '_>,
        _: Context<'_, Self::State>,
    ) {
        if ident == "search" {
            let str = value.to_common_str().as_ref();
        }
    }
}
