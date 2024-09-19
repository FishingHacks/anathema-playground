use anathema::component::*;
use anathema::widgets::components::events::KeyState;

use crate::editor::THREAD_HANDLE;

#[derive(State, Debug)]
pub struct InputState {
    name: Value<String>,
    input: Value<String>,
    focused: Value<bool>,
    position_x: Value<usize>,
}

impl InputState {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string().into(),
            input: Default::default(),
            focused: Default::default(),
            position_x: Default::default(),
        }
    }
}

#[derive(Default)]
pub struct Input;

impl Component for Input {
    type State = InputState;
    type Message = ();

    fn on_blur(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        _: anathema::prelude::Context<'_, Self::State>,
    ) {
        state.focused.set(false);
    }

    fn on_focus(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        _: anathema::prelude::Context<'_, Self::State>,
    ) {
        state.focused.set(true);
    }

    fn on_key(
        &mut self,
        key: KeyEvent,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        mut context: anathema::prelude::Context<'_, Self::State>,
    ) {
        if THREAD_HANDLE.with_borrow(|v| v.is_some()) || matches!(key.state, KeyState::Release) {
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.ctrl => state.input.to_mut().clear(),
            KeyCode::Char(c) => {
                if state.position_x.copy_value() > state.input.to_ref().len() {
                    state.position_x.set(state.input.to_ref().len());
                }
                let pos_x = state.position_x.copy_value();
                if pos_x == state.input.to_ref().len() {
                    state.input.to_mut().push(c);
                } else {
                    state.input.to_mut().insert(pos_x, c);
                }
                state.position_x.set(pos_x + 1);
            }
            KeyCode::Backspace => {
                if state.position_x.copy_value() > state.input.to_ref().len() {
                    state.position_x.set(state.input.to_ref().len());
                }
                let pos_x = state.position_x.copy_value();
                if pos_x == 0 {
                    return;
                } else if pos_x == state.input.to_ref().len() {
                    _ = state.input.to_mut().pop();
                } else {
                    _ = state.input.to_mut().remove(pos_x - 1);
                }
                state.position_x.set(pos_x - 1);
            }
            KeyCode::Delete => {
                if state.position_x.copy_value() > state.input.to_ref().len() {
                    state.position_x.set(state.input.to_ref().len());
                }
                let pos_x = state.position_x.copy_value();
                if pos_x == state.input.to_ref().len() {
                    return;
                } else {
                    _ = state.input.to_mut().remove(pos_x);
                }
            }
            KeyCode::Left => {
                let pos_x = state.position_x.copy_value();
                if pos_x > state.input.to_ref().len() {
                    state.position_x.set(state.input.to_ref().len());
                } else if pos_x > 0 {
                    state.position_x.set(pos_x - 1);
                }
            }
            KeyCode::Right => {
                let pos_x = state.position_x.copy_value();
                if pos_x >= state.input.to_ref().len() {
                    state.position_x.set(state.input.to_ref().len());
                } else {
                    state.position_x.set(pos_x + 1);
                }
            }
            KeyCode::Up => state.position_x.set(0),
            KeyCode::Down => state.position_x.set(state.input.to_ref().len()),
            KeyCode::Enter => context.publish("submit", |state| &state.input),
            _ => (),
        }
    }

    fn accept_focus(&self) -> bool {
        THREAD_HANDLE.with_borrow(|v| v.is_none())
    }
}
