use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use anathema::backend::{
    tui::{Buffer, Style},
    Backend,
};
use anathema::geometry::{LocalPos, Pos, Size};
use anathema::prelude::Document;
use anathema::resolver::Attributes;
use anathema::runtime::Runtime;
use anathema::widgets::components::events::Event;
use anathema::widgets::{paint::Glyph, GlyphMap, WidgetRenderer};

struct BufferRender<'a>(&'a mut Buffer);

impl WidgetRenderer for BufferRender<'_> {
    fn draw_glyph(&mut self, glyph: Glyph, local_pos: anathema::geometry::Pos) {
        let Ok(screen_pos) = local_pos.try_into() else {
            return;
        };
        self.0.put_glyph(glyph, screen_pos);
    }

    fn set_attributes(&mut self, attribs: &Attributes<'_>, local_pos: Pos) {
        let Ok(screen_pos) = local_pos.try_into() else {
            return;
        };
        let style = Style::from_cell_attribs(attribs);
        self.0.update_cell(style, screen_pos);
    }

    fn size(&self) -> Size {
        self.0.size()
    }

    fn set_style(&mut self, style: Style, local_pos: Pos) {
        let Ok(pos) = local_pos.try_into() else {
            return;
        };
        self.0.update_cell(style, pos);
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
            Ok(ThreadEvent::Resize { width, height }) => {
                Some(Event::Resize(Size::new(width, height)))
            }
        }
    }

    fn resize(&mut self, new_size: Size, glyph_map: &mut GlyphMap) {
        self.clear();

        self.render(glyph_map);

        self.buffer.resize(new_size);
    }

    fn paint<'bp>(
        &mut self,
        glyph_map: &mut GlyphMap,
        widgets: anathema::widgets::PaintChildren<'_, 'bp>,
        attribute_storage: &anathema::resolver::AttributeStorage<'bp>,
    ) {
        anathema::widgets::paint::paint(
            &mut BufferRender(&mut self.buffer),
            glyph_map,
            widgets,
            attribute_storage,
        );
    }

    fn render(&mut self, glyph_map: &mut GlyphMap) {
        let size = self.buffer.size();
        let mut rendered_buffer = RenderedBuffer::create(size.width as usize, size.height as usize);

        for x in 0..size.width {
            for y in 0..size.height {
                if let Some((&glyph, &style)) = self.buffer.get((x, y).into()) {
                    match glyph {
                        Glyph::Single(c, _) => {
                            rendered_buffer.set_at(x as usize, y as usize, c, style)
                        }
                        Glyph::Cluster(idx, _) => {
                            if let Some(value) = glyph_map.get(idx) {
                                for (offset_x, character) in value.chars().enumerate() {
                                    rendered_buffer.set_at(
                                        x as usize + offset_x,
                                        y as usize,
                                        character,
                                        style,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        if self.buffer_sender.send(rendered_buffer).is_err() {
            panic!("failed to send updates")
        }
    }

    fn clear(&mut self) {
        let width = self.size().width;
        let height = self.size().height;

        for x in 0..width {
            for y in 0..height {
                let Some((glyph, style)) = self.buffer.get_mut(LocalPos::new(x, y)) else {
                    continue;
                };
                *glyph = Glyph::space();
                *style = Style::reset();
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
        let mut backend = ThreadBackend {
            buffer: Buffer::new(initial_size),
            buffer_sender,
            event_receiver,
        };

        Runtime::builder(document, &backend)
            .finish(|v| v.run(&mut backend))
            .expect("we should never fail to compile the document")
    })?;

    Ok(AnathemaThreadHandle {
        buffer_receiver,
        event_sender,
        thread_handle,
    })
}
