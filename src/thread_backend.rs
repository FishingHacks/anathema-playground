use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use anathema::backend::tui::{Buffer, Style};
use anathema::backend::Backend;
use anathema::geometry::{LocalPos, Pos, Size};
use anathema::prelude::Document;
use anathema::runtime::Runtime;
use anathema::widgets::components::events::Event;
use anathema::widgets::paint::CellAttributes;
use anathema::widgets::WidgetRenderer;

struct BufferRender<'a>(&'a mut Buffer);

impl WidgetRenderer for BufferRender<'_> {
    fn draw_glyph(&mut self, c: char, local_pos: anathema::geometry::Pos) {
        let Ok(screen_pos) = local_pos.try_into() else {
            return;
        };
        self.0.put_char(c, screen_pos);
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

pub struct ThreadBackend {
    buffer_sender: Sender<Buffer>,
    event_receiver: Receiver<ThreadEvent>,
    buf_a: Buffer,
    buf_b: Buffer,
    using_a: bool,
}

impl Backend for ThreadBackend {
    fn size(&self) -> Size {
        self.buf_a.size() // both buffers are the same size
    }

    fn quit_test(&self, _: Event) -> bool {
        true
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
        self.buf_a.resize(new_size);
        self.buf_b.resize(new_size);
    }

    fn paint<'bp>(
        &mut self,
        element: &mut anathema::widgets::Element<'bp>,
        children: &[anathema::store::tree::Node],
        values: &mut anathema::store::tree::TreeValues<anathema::widgets::WidgetKind<'bp>>,
        text: &mut anathema::widgets::layout::text::StringSession<'_>,
        attribute_storage: &anathema::widgets::AttributeStorage<'bp>,
        ignore_floats: bool,
    ) {
        anathema::widgets::paint::paint(
            &mut BufferRender(if self.using_a {
                &mut self.buf_a
            } else {
                &mut self.buf_b
            }),
            element,
            children,
            values,
            attribute_storage,
            text,
            ignore_floats,
        )
    }

    fn render(&mut self) {
        match self.buffer_sender.send(if self.using_a {
            self.buf_b.clone()
        } else {
            self.buf_a.clone()
        }) {
            Err(_) => panic!("failed to send updates"),
            Ok(_) => self.using_a = !self.using_a,
        }
    }

    fn clear(&mut self) {
        let width = self.size().width as u16;
        let height = self.size().height as u16;

        let buf = if self.using_a {
            &mut self.buf_a
        } else {
            &mut self.buf_b
        };
        for x in 0..width {
            for y in 0..height {
                buf.empty(LocalPos::new(x, y));
            }
        }
    }
}

pub struct AnathemaThreadHandle {
    thread_handle: JoinHandle<()>,
    buffer_receiver: Receiver<Buffer>,
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

    pub fn get_buffer(&self) -> Result<Option<Buffer>, ()> {
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
        let document = Document::new(document);
        let backend = ThreadBackend {
            buf_a: Buffer::new(initial_size),
            buf_b: Buffer::new(initial_size),
            buffer_sender,
            event_receiver,
            using_a: true,
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
