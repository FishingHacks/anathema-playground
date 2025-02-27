use std::{path::PathBuf, time::Duration};

use anathema::{component::*, default_widgets::Canvas, prelude::*};
use editor::{Editor, EditorState, THREAD_HANDLE};
use input::{Input, InputState};
use thread_backend::AnathemaThreadHandle;

mod editor;
mod input;
mod text_buffer;
mod thread_backend;

struct Playground(ComponentId<()>);

//#[derive(State)]
enum Showing {
    Editor,
    Preview,
}
impl anathema::component::State for Showing {
    fn type_info(&self) -> anathema::state::Type {
        anathema::state::Type::String
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Showing::Editor => Some("Editor"),
            Showing::Preview => Some("Preview"),
        }
    }
}

#[derive(State)]
struct PlaygroundState {
    showing: Value<Showing>,
    width: Value<usize>,
    height: Value<usize>,
}

impl Component for Playground {
    type Message = ();
    type State = PlaygroundState;

    fn receive(
        &mut self,
        ident: &str,
        _: &dyn AnyState,
        state: &mut Self::State,
        mut elements: Children<'_, '_>,
        mut context: Context<'_, '_, Self::State>,
    ) {
        if ident == "run_aml" {
            elements
                .elements()
                .by_tag("canvas")
                .by_attribute("id", "preview")
                .first(|element, _| {
                    let canvas_size = element.size();
                    let Some(canvas) = element.try_to::<Canvas>() else {
                        return;
                    };

                    for y in 0..canvas_size.height {
                        for x in 0..canvas_size.width {
                            canvas.erase((x, y));
                        }
                    }
                });
            *state.showing.to_mut() = Showing::Preview;
            context.components.by_name("main").focus();
        }
    }

    fn on_blur(
        &mut self,
        state: &mut Self::State,
        _: Children<'_, '_>,
        context: Context<'_, '_, Self::State>,
    ) {
        if let Some(handle) = THREAD_HANDLE.take() {
            handle.close();
        }
        *state.showing.to_mut() = Showing::Editor;
        context.emit(self.0, ());
    }

    fn tick(
        &mut self,
        _: &mut Self::State,
        mut elements: Children<'_, '_>,
        _: Context<'_, '_, Self::State>,
        _: Duration,
    ) {
        let maybe_buffer = THREAD_HANDLE.with_borrow_mut(|maybe_handle| {
            if let Some(handle) = maybe_handle {
                match handle.get_buffer() {
                    Err(_) => {
                        _ = maybe_handle.take().map(AnathemaThreadHandle::close);
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
            .elements()
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
                            let (char, style) = buffer.get(x as usize, y as usize);
                            if *char != '\0' {
                                canvas.put(*char, *style, (x, y));
                            } else {
                                canvas.erase((x, y));
                            }
                        }
                    }
                }
            });
    }

    fn resize(
        &mut self,
        state: &mut Self::State,
        _: Children<'_, '_>,
        context: Context<'_, '_, Self::State>,
    ) {
        let size = context.viewport.size();
        *state.width.to_mut() = size.width as usize;
        *state.height.to_mut() = size.height as usize;

        THREAD_HANDLE.with_borrow_mut(|maybe_handle| {
            if let Some(handle) = maybe_handle {
                if handle.resize(size.width, size.height).is_err() {
                    _ = maybe_handle.take().map(AnathemaThreadHandle::close);
                }
            }
        });
    }

    fn on_focus(
        &mut self,
        _: &mut Self::State,
        _: Children<'_, '_>,
        mut context: Context<'_, '_, Self::State>,
    ) {
        if THREAD_HANDLE.with_borrow(|maybe_handle| maybe_handle.is_none()) {
            context.components.by_name("editor").focus();
        }
    }

    fn accept_focus(&self) -> bool {
        true
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

    let file = match std::env::args().nth(1) {
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

    let mut backend = TuiBackend::builder()
        //.enable_alt_screen()
        .enable_raw_mode()
        .hide_cursor()
        .finish()
        .expect("failed to build the backend");
    backend.finalize();

    let mut editor_size = backend.size();
    editor_size.width -= 2;
    editor_size.height -= 2;
    let size = backend.size();

    let mut runtime = Runtime::builder(Document::new("@main"), &backend);
    let editor_state = EditorState::new(editor_size, file.as_deref());
    runtime
        .component(
            "input",
            release_bundle!("templates/input.aml"),
            Input,
            InputState::new("Search"),
        )
        .unwrap();
    let editor = runtime
        .component(
            "editor",
            release_bundle!("templates/editor.aml"),
            Editor::new(file, editor_size),
            editor_state,
        )
        .unwrap();

    runtime
        .component(
            "main",
            release_bundle!("templates/main.aml"),
            Playground(editor),
            PlaygroundState {
                showing: Showing::Editor.into(),
                width: Value::new(size.width as usize),
                height: Value::new(size.width as usize),
            },
        )
        .unwrap();

    runtime.fps(60);
    runtime.finish(|v| v.run(&mut backend)).unwrap();
}
