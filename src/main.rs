use std::path::PathBuf;

use anathema::{
    backend::Backend, component::*, default_widgets::Canvas, prelude::*,
    widgets::components::events::KeyState,
};
use editor::{Editor, EditorState, ERROR, THREAD_HANDLE};
use thread_backend::AnathemaThreadHandle;

mod editor;
mod text_buffer;
mod thread_backend;

struct Playground(ComponentId<()>);

#[derive(State)]
enum Showing {
    Editor,
    Error { errors: Value<List<String>> },
    Preview,
}

#[derive(State)]
struct PlaygroundState {
    showing: Value<Showing>,
    focused: Value<bool>,
    width: Value<usize>,
    height: Value<usize>,
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
            if let Some(handle) = THREAD_HANDLE.take() {
                handle.close();
            }
            *state.showing.to_mut() = Showing::Editor;
            _ = ctx.emit(self.0, ());
        }
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

    fn accept_focus(&self) -> bool {
        true
    }

    fn tick(
        &mut self,
        _: &mut Self::State,
        mut elements: Elements<'_, '_>,
        _: Context<'_, Self::State>,
        _: std::time::Duration,
    ) {
        let maybe_buffer = THREAD_HANDLE.with_borrow_mut(|maybe_handle| {
            if let Some(handle) = maybe_handle {
                match handle.get_buffer() {
                    Err(_) => {
                        println!("close");
                        maybe_handle.take().map(AnathemaThreadHandle::close);
                        None
                    }
                    Ok(v) => v,
                }
            } else {
                None
            }
        });
        let Some(buffer) = maybe_buffer else {
            return;
        };

        elements
            .by_tag("canvas")
            .by_attribute("id", "preview")
            .first(|element, _| {
                let canvas_size = element.size();
                let buffer_size = buffer.size();
                let Some(canvas) = element.try_to::<Canvas>() else {
                    return;
                };

                for y in 0..canvas_size.height {
                    for x in 0..canvas_size.width {
                        if x < buffer_size.width && y < buffer_size.height {
                            if let Some((char, style)) = buffer.get((x as u16, y as u16).into()) {
                                canvas.put(*char, *style, (x as u16, y as u16));
                                continue;
                            }
                        }
                        canvas.erase((x as u16, y as u16));
                    }
                }
            });
    }

    fn resize(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        context: Context<'_, Self::State>,
    ) {
        let size = context.viewport.size();
        *state.width.to_mut() = size.width;
        *state.height.to_mut() = size.height;

        THREAD_HANDLE.with_borrow_mut(|maybe_handle| {
            if let Some(handle) = maybe_handle {
                if let Err(_) = handle.resize(size.width as u16, size.height as u16) {
                    println!("close");
                    maybe_handle.take().map(AnathemaThreadHandle::close);
                }
            }
        });
    }
}

macro_rules! release_bundle {
    ($path: expr) => {{
        #[cfg(debug_assertions)]
        {
            ($path as &'static str).to_path()
        }
        #[cfg(not(debug_assertions))]
        {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/", $path)).to_template()
        }
    }};
}

fn main() {
    let mut current_executable =
        std::env::current_exe().expect("Failed to get path to the current executable");
    if let Ok(path) = current_executable.strip_prefix(std::env::current_dir().unwrap_or_default()) {
        current_executable = path.to_path_buf();
    }

    let file = match std::env::args().skip(1).next() {
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
            Some(path)
        }
        _ => None,
    };

    let backend = TuiBackend::builder()
        .enable_alt_screen()
        .enable_raw_mode()
        .hide_cursor()
        .finish()
        .expect("failed to build the backend");

    let mut editor_size = backend.size();
    editor_size.width -= 2;
    editor_size.height -= 2;
    let size = backend.size();

    let mut runtime = Runtime::builder(Document::new("@main"), backend);
    runtime
        .register_default::<()>("error", release_bundle!("templates/error.aml"))
        .unwrap();

    let editor_state = EditorState::new(editor_size, file.as_ref().map(PathBuf::as_path));
    let editor = runtime
        .register_component(
            "editor",
            release_bundle!("templates/editor.aml"),
            Editor::new(file, editor_size),
            editor_state,
        )
        .unwrap();

    runtime
        .register_component(
            "main",
            release_bundle!("templates/main.aml"),
            Playground(editor),
            PlaygroundState {
                showing: Showing::Editor.into(),
                focused: true.into(),
                width: size.width.into(),
                height: size.height.into(),
            },
        )
        .unwrap();

    let mut rt = runtime.finish().expect("failed to build the runtime");
    rt.fps = 60;
    rt.run();
}
