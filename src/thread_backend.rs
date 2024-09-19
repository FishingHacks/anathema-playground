use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use anathema::backend::tui::{Buffer, Style};
use anathema::backend::Backend;
use anathema::geometry::{LocalPos, Pos, Size};
use anathema::prelude::Document;
use anathema::runtime::Runtime;
use anathema::widgets::components::events::Event;
use anathema::widgets::paint::{CellAttributes, Glyph};
use anathema::widgets::{GlyphMap, WidgetRenderer};

struct BufferRender<'a>(&'a mut Buffer);

impl WidgetRenderer for BufferRender<'_> {
    fn draw_glyph(&mut self, glyph: Glyph, local_pos: anathema::geometry::Pos) {
        let Ok(screen_pos) = local_pos.try_into() else {
            return;
        };
        self.0.put_glyph(glyph, screen_pos);
    }

    fn set_attributes(&mut self, attribs: &dyn CellAttributes, pos: Pos) {
        let Ok(screen_pos) = pos.try_into() else {
            return;
        };
        let style = Style::from_cell_attribs(attribs);
        self.0.update_cell(style, screen_pos);
    }

    fn size(&self) -> Size {
        self.0.size()
    }
}

pub enum ThreadEvent {
    Quit,
    Resize { width: u16, height: u16 },
}

#[derive(Clone)]
pub struct RenderedBuffer {
    value: Box<[(char, Style)]>,
    width: usize,
    height: usize,
}

impl RenderedBuffer {
    pub fn size(&self) -> Size {
        (self.width, self.height).into()
    }

    pub fn get(&self, x: usize, y: usize) -> &(char, Style) {
        &self.value[x + y * self.width]
    }

    pub(crate) fn set_at(&mut self, x: usize, y: usize, character: char, style: Style) {
        self.value[x + y * self.width] = (character, style);
    }

    pub(crate) fn create(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            value: vec![('\0', Style::reset()); width * height].into_boxed_slice(),
        }
    }
}

pub struct ThreadBackend {
    buffer_sender: Sender<RenderedBuffer>,
    event_receiver: Receiver<ThreadEvent>,
    buffer: Buffer,
}

impl Backend for ThreadBackend {
    fn size(&self) -> Size {
        self.buffer.size() // both buffers are the same size
    }

    fn next_event(&mut self, _: std::time::Duration) -> Option<Event> {
        match self.event_receiver.try_recv() {
            Err(TryRecvError::Empty) => None,
            Err(_) => Some(Event::Stop), // if the connection is closed, close the thread
            Ok(ThreadEvent::Quit) => Some(Event::Stop),
            Ok(ThreadEvent::Resize { width, height }) => Some(Event::Resize(width, height)),
        }
    }

    fn resize(&mut self, new_size: Size) {
        self.buffer.resize(new_size);
    }

    fn paint<'bp>(
        &mut self,
        glyph_map: &mut GlyphMap,
        element: &mut anathema::widgets::Element<'bp>,
        children: &[anathema::store::tree::Node],
        values: &mut anathema::store::tree::TreeValues<anathema::widgets::WidgetKind<'bp>>,
        attribute_storage: &anathema::widgets::AttributeStorage<'bp>,
        ignore_floats: bool,
    ) {
        anathema::widgets::paint::paint(
            &mut BufferRender(&mut self.buffer),
            glyph_map,
            element,
            children,
            values,
            attribute_storage,
            ignore_floats,
        )
    }

    fn render(&mut self, glyph_map: &mut GlyphMap) {
        let size = self.buffer.size();
        let mut rendered_buffer = RenderedBuffer::create(size.width, size.height);

        for x in 0..size.width {
            for y in 0..size.height {
                if let Some((&glyph, &style)) = self.buffer.get((x as u16, y as u16).into()) {
                    match glyph {
                        Glyph::Single(c, _) => rendered_buffer.set_at(x, y, c, style),
                        Glyph::Cluster(idx, _) => {
                            if let Some(value) = glyph_map.get(idx) {
                                for (offset_x, character) in value.chars().enumerate() {
                                    rendered_buffer.set_at(x + offset_x, y, character, style);
                                }
                            }
                        }
                    }
                }
            }
        }

        match self.buffer_sender.send(rendered_buffer) {
            Err(_) => panic!("failed to send updates"),
            Ok(_) => (),
        }
    }

    fn clear(&mut self) {
        let width = self.size().width as u16;
        let height = self.size().height as u16;

        for x in 0..width {
            for y in 0..height {
                self.buffer.empty(LocalPos::new(x, y));
            }
        }
    }
}

pub struct AnathemaThreadHandle {
    thread_handle: JoinHandle<()>,
    buffer_receiver: Receiver<RenderedBuffer>,
    event_sender: Sender<ThreadEvent>,
}

impl AnathemaThreadHandle {
    pub fn close(self) {
        _ = self.event_sender.send(ThreadEvent::Quit);
        _ = self.thread_handle.join();
    }

    pub fn resize(&mut self, new_width: u16, new_height: u16) -> Result<(), ()> {
        self.event_sender
            .send(ThreadEvent::Resize {
                width: new_width,
                height: new_height,
            })
            .map_err(|_| ())
    }

    pub fn get_buffer(&self) -> Result<Option<RenderedBuffer>, ()> {
        match self.buffer_receiver.try_recv() {
            Ok(v) => Ok(Some(v)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(_) => Err(()),
        }
    }
}

pub fn launch_threaded_anathema(
    document: String,
    initial_size: Size,
) -> Result<AnathemaThreadHandle, std::io::Error> {
    let (buffer_sender, buffer_receiver) = channel();
    let (event_sender, event_receiver) = channel();

    let thread_handle = std::thread::Builder::new().spawn(move || {
        let panic_sender = buffer_sender.clone();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info.payload();
            let str = if let Some(&s) = payload.downcast_ref::<&'static str>() {
                s
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.as_str()
            } else {
                "Box<dyn Any>"
            };
            let panic_prefix = "Panic: ";
            let mut width = 0;
            let mut height = 3;
            let mut last_width = panic_prefix.len();
            for char in str.chars() {
                if char == '\n' {
                    width = width.max(last_width);
                    last_width = 0;
                    height += 1;
                } else {
                    last_width += 1;
                }
            }
            width = width.max(last_width);

            let mut buffer = RenderedBuffer::create(width, height);

            for (x, c) in panic_prefix.chars().enumerate() {
                buffer.set_at(x, 1, c, Style::reset());
            }

            for (y, line) in str.lines().enumerate() {
                for (x, c) in line.chars().enumerate() {
                    if c == '\n' {
                        break;
                    }
                    if y == 0 {
                        buffer.set_at(x + panic_prefix.len(), y + 1, c, Style::reset());
                    } else {
                        buffer.set_at(x, y + 1, c, Style::reset());
                    }
                }
            }

            _ = panic_sender.send(buffer);
            //std::process::abort();
        }));
        let document = Document::new(document);
        let backend = ThreadBackend {
            buffer: Buffer::new(initial_size),
            buffer_sender,
            event_receiver,
        };

        Runtime::builder(document, backend)
            .finish()
            .expect("we should never fail to compile the document")
            .run();
    })?;

    Ok(AnathemaThreadHandle {
        buffer_receiver,
        event_sender,
        thread_handle,
    })
}
