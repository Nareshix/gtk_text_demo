use gtk4::{glib, prelude::*};
use gtk4::{
    Application, ApplicationWindow,
    TextView, TextBuffer, TextTagTable, TextTag,
    ScrolledWindow, WrapMode,
};
use gtk4::Box as GtkBox;
use std::cell::RefCell;
use std::rc::Rc;

// ═══════════════════════════════════════════════
//  MODEL
// ═══════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
enum ParagraphKind {
    Normal,
    Heading(u8),
}

#[derive(Debug, Clone)]
struct Paragraph {
    kind: ParagraphKind,
    text: String,
}

impl Paragraph {
    fn new(kind: ParagraphKind, text: &str) -> Self {
        Self { kind, text: text.to_string() }
    }
}

#[derive(Debug, Clone)]
struct Document {
    paragraphs: Vec<Paragraph>,
    // cursor = which paragraph + offset within that paragraph's text
    cursor_para: usize,
    cursor_offset: usize,
}

impl Document {
    fn new() -> Self {
        Self {
            paragraphs: vec![Paragraph::new(ParagraphKind::Normal, "")],
            cursor_para: 0,
            cursor_offset: 0,
        }
    }

    // insert a character at cursor
    fn insert_char(&mut self, ch: char) {
        let para = &mut self.paragraphs[self.cursor_para];
        para.text.insert(self.cursor_offset, ch);
        self.cursor_offset += ch.len_utf8();
    }

    // delete character before cursor (backspace)
    fn backspace(&mut self) {
        if self.cursor_offset > 0 {
            // delete char before cursor in same paragraph
            let para = &mut self.paragraphs[self.cursor_para];
            let ch_start = para.text[..self.cursor_offset]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            para.text.remove(ch_start);
            self.cursor_offset = ch_start;
        } else if self.cursor_para > 0 {
            // at start of paragraph — merge with previous
            let current_text = self.paragraphs[self.cursor_para].text.clone();
            let prev = &mut self.paragraphs[self.cursor_para - 1];
            let new_offset = prev.text.len();
            prev.text.push_str(&current_text);
            self.paragraphs.remove(self.cursor_para);
            self.cursor_para -= 1;
            self.cursor_offset = new_offset;
        }
    }

    // split paragraph at cursor (Enter)
    fn split(&mut self) {
        let para = &mut self.paragraphs[self.cursor_para];
        let rest = para.text[self.cursor_offset..].to_string();
        para.text.truncate(self.cursor_offset);

        // new paragraph gets Normal kind regardless of current kind
        // headings dont continue on Enter — same as every editor
        let new_para = Paragraph::new(ParagraphKind::Normal, &rest);
        self.paragraphs.insert(self.cursor_para + 1, new_para);
        self.cursor_para += 1;
        self.cursor_offset = 0;
    }

    // compute the absolute buffer offset for the cursor
    // renderer needs this to place the GTK cursor
    fn cursor_buffer_offset(&self) -> i32 {
        let mut offset = 0i32;
        for (i, para) in self.paragraphs.iter().enumerate() {
            if i == self.cursor_para {
                offset += self.cursor_offset as i32;
                break;
            }
            // +1 for the newline separator between paragraphs
            offset += para.text.len() as i32 + 1;
        }
        offset
    }
}

// ═══════════════════════════════════════════════
//  RENDERER
// ═══════════════════════════════════════════════

fn render(doc: &Document, buffer: &TextBuffer) {
    // wipe buffer
    let (mut s, mut e) = buffer.bounds();
    buffer.delete(&mut s, &mut e);

    let count = doc.paragraphs.len();
    for (i, para) in doc.paragraphs.iter().enumerate() {
        let para_start = buffer.end_iter().offset();

        // insert text
        let mut iter = buffer.end_iter();
        buffer.insert(&mut iter, &para.text);

        // apply paragraph tag BEFORE newline
        match para.kind {
            ParagraphKind::Heading(1) => {
                buffer.apply_tag_by_name(
                    "h1",
                    &buffer.iter_at_offset(para_start),
                    &buffer.end_iter(),
                );
            }
            ParagraphKind::Heading(2) => {
                buffer.apply_tag_by_name(
                    "h2",
                    &buffer.iter_at_offset(para_start),
                    &buffer.end_iter(),
                );
            }
            _ => {}
        }

        // newline AFTER tags — no bleeding
        if i < count - 1 {
            let mut iter = buffer.end_iter();
            buffer.insert(&mut iter, "\n");
        }
    }

    // place GTK cursor to match model cursor
    let offset = doc.cursor_buffer_offset();
    let iter = buffer.iter_at_offset(offset);
    buffer.place_cursor(&iter);
}

// ═══════════════════════════════════════════════
//  MAIN
// ═══════════════════════════════════════════════

fn main() {
    let app = Application::builder()
        .application_id("com.example.demo")
        .build();
    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {

    // ── tags ───────────────────────────────────
    let tag_table = TextTagTable::new();

    let h1 = TextTag::new(Some("h1"));
    h1.set_size_points(26.0);
    h1.set_weight(800);
    tag_table.add(&h1);

    let h2 = TextTag::new(Some("h2"));
    h2.set_size_points(20.0);
    h2.set_weight(700);
    tag_table.add(&h2);

    // ── buffer ─────────────────────────────────
    let buffer = TextBuffer::new(Some(&tag_table));

    // ── document — wrapped in Rc<RefCell> ──────
    // Rc = shared ownership between closures
    // RefCell = interior mutability (mutate inside Fn closures)
    let doc = Rc::new(RefCell::new(Document::new()));

    // initial render
    render(&doc.borrow(), &buffer);

    // ── textview ───────────────────────────────
    let textview = TextView::builder()
        .buffer(&buffer)
        .wrap_mode(WrapMode::Word)
        .editable(false)     // GTK never writes to buffer
        .left_margin(32)
        .right_margin(32)
        .top_margin(24)
        .bottom_margin(24)
        .pixels_above_lines(2)
        .pixels_below_lines(2)
        .build();

    // ── key controller ─────────────────────────
    // ALL input goes through here
    // every keystroke → update model → render → screen
    let controller = gtk4::EventControllerKey::new();
    controller.connect_key_pressed({
        let doc = doc.clone();
        let buffer = buffer.clone();
        move |_, key, _, modifier| {

            // ignore ctrl shortcuts for now
            if modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }

            let mut doc = doc.borrow_mut();

            match key {
                // enter — split paragraph
                gtk4::gdk::Key::Return => {
                    doc.split();
                    render(&doc, &buffer);
                    glib::Propagation::Stop
                }

                // backspace
                gtk4::gdk::Key::BackSpace => {
                    doc.backspace();
                    render(&doc, &buffer);
                    glib::Propagation::Stop
                }

                // arrow keys — move cursor through model
                gtk4::gdk::Key::Right => {
                    let para = &doc.paragraphs[doc.cursor_para];
                    if doc.cursor_offset < para.text.len() {
                        doc.cursor_offset += 1;
                    } else if doc.cursor_para < doc.paragraphs.len() - 1 {
                        doc.cursor_para += 1;
                        doc.cursor_offset = 0;
                    }
                    render(&doc, &buffer);
                    glib::Propagation::Stop
                }

                gtk4::gdk::Key::Left => {
                    if doc.cursor_offset > 0 {
                        doc.cursor_offset -= 1;
                    } else if doc.cursor_para > 0 {
                        doc.cursor_para -= 1;
                        doc.cursor_offset = doc.paragraphs[doc.cursor_para].text.len();
                    }
                    render(&doc, &buffer);
                    glib::Propagation::Stop
                }

                // any printable character
                _ => {
                    if let Some(ch) = key.to_unicode() {
                        if !ch.is_control() {
                            doc.insert_char(ch);
                            render(&doc, &buffer);
                            return glib::Propagation::Stop;
                        }
                    }
                    glib::Propagation::Proceed
                }
            }
        }
    });
    textview.add_controller(controller);

    // ── heading button ─────────────────────────
    // shows how a toolbar action updates the model
    let btn = gtk4::Button::with_label("Toggle H1 on current paragraph");
   btn.connect_clicked({
    let doc = doc.clone();
    let buffer = buffer.clone();
    move |_| {
        let mut doc = doc.borrow_mut();
        let idx = doc.cursor_para;        // read this first
        let para = &mut doc.paragraphs[idx];  // now only one borrow
        para.kind = match para.kind {
            ParagraphKind::Heading(1) => ParagraphKind::Normal,
            _                         => ParagraphKind::Heading(1),
        };
        render(&doc, &buffer);
    }
});
    // ── layout ─────────────────────────────────
    let scrolled = ScrolledWindow::builder()
        .child(&textview)
        .vexpand(true)
        .build();

    let vbox = GtkBox::new(gtk4::Orientation::Vertical, 4);
    vbox.append(&btn);
    vbox.append(&scrolled);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Document → Renderer → Buffer")
        .default_width(700)
        .default_height(500)
        .child(&vbox)
        .build();

    window.present();
}