use anathema::component::*;
use anathema::widgets::components::events::KeyState;

use crate::editor::THREAD_HANDLE;

#[derive(State, Debug)]
pub struct InputState {
    name: Value<String>,
    input: Value<String>,
    focused: Value<bool>,
}

impl InputState {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string().into(),
            input: Default::default(),
            focused: Default::default(),
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
        if THREAD_HANDLE.with_borrow(|v| v.is_some()) || matches!(key.state, KeyState::Release) { return }

        match key.code {
            KeyCode::Char('c') if key.ctrl => state.input.to_mut().clear(),
            KeyCode::Char(c) => state.input.to_mut().push(c),
            KeyCode::Backspace => _ = state.input.to_mut().pop(),
            KeyCode::Enter => context.publish("submit", |state| &state.input),
            _ => ()
        }
    }

    fn accept_focus(&self) -> bool {
        THREAD_HANDLE.with_borrow(|v| v.is_none())
    }
}
