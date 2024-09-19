use std::path::PathBuf;

use anathema::{backend::Backend, component::*, default_widgets::Canvas, prelude::*};
use editor::{Editor, EditorState, THREAD_HANDLE};
use input::{Input, InputState};
use thread_backend::AnathemaThreadHandle;

mod editor;
mod input;
mod text_buffer;
mod thread_backend;

struct Playground(ComponentId<()>);

#[derive(State)]
enum Showing {
    Editor,
    Preview,
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
        _: CommonVal<'_>,
        state: &mut Self::State,
        mut elements: Elements<'_, '_>,
        mut ctx: Context<'_, Self::State>,
    ) {
        if ident == "run_aml" {
            elements
                .by_tag("canvas")
                .by_attribute("id", "preview")
                .first(|element, _| {
                    let canvas_size = element.size();
                    let Some(canvas) = element.try_to::<Canvas>() else {
                        return;
                    };

                    for y in 0..canvas_size.height as u16 {
                        for x in 0..canvas_size.width as u16 {
                            canvas.erase((x, y));
                        }
                    }
                });
            *state.showing.to_mut() = Showing::Preview;
            ctx.set_focus("id", "main");
        }
    }

    fn on_blur(
        &mut self,
        state: &mut Self::State,
        _: Elements<'_, '_>,
        ctx: Context<'_, Self::State>,
    ) {
        if let Some(handle) = THREAD_HANDLE.take() {
            handle.close();
        }
        *state.showing.to_mut() = Showing::Editor;
        _ = ctx.emit(self.0, ());
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
                            let (char, style) = buffer.get(x, y);
                            if *char != '\0' {
                                canvas.put(*char, *style, (x as u16, y as u16));
                            } else {
                                canvas.erase((x as u16, y as u16));
                            }
                        }
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
                    maybe_handle.take().map(AnathemaThreadHandle::close);
                }
            }
        });
    }

    fn on_focus(
        &mut self,
        _: &mut Self::State,
        _: Elements<'_, '_>,
        mut context: Context<'_, Self::State>,
    ) {
        if THREAD_HANDLE.with_borrow(|maybe_handle| maybe_handle.is_none()) {
            context.set_focus("id", "editor");
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

    let mut runtime = Runtime::builder(Document::new("@main [id: \"main\"]"), backend);
    let editor_state = EditorState::new(editor_size, file.as_ref().map(PathBuf::as_path));
    runtime
        .register_component(
            "input",
            release_bundle!("templates/input.aml"),
            Input,
            InputState::new("Search"),
        )
        .unwrap();
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
                width: size.width.into(),
                height: size.height.into(),
            },
        )
        .unwrap();

    let runtime = runtime.set_global_event_handler(GlobalEventHandler);

    let mut rt = runtime.finish().expect("failed to build the runtime");
    rt.fps = 60;
    rt.run();
}

struct GlobalEventHandler;

impl GlobalEvents for GlobalEventHandler {
    // do manual tabbing
    fn enable_tab_navigation(&mut self) -> bool {
        true
    }

    fn handle(
        &mut self,
        event: Event,
        _: &mut Elements<'_, '_>,
        _: &mut GlobalContext<'_>,
    ) -> Option<Event> {
        Some(event)
    }
}
