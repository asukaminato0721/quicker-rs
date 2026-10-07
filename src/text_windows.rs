//! Native workflow text windows. The UI thread owns viewports. Action workers
//! change shared documents and can wait without blocking the UI thread.
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, Context, ViewportId};

pub(crate) const TEXT_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub title: String,
    pub text: String,
    pub key: String,
    pub size: [f32; 2],
    pub font_size: f32,
    pub top_most: bool,
    pub wrap: bool,
    pub line_numbers: bool,
    pub toolbar: bool,
    pub escape_close: bool,
    pub close_on_blur: bool,
    pub caret: i64,
    pub background: Option<Color32>,
    pub foreground: Option<Color32>,
    pub operations: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    pub text: String,
    pub selected: String,
    pub caret: usize,
    pub operation: String,
    pub position: String,
    pub closed: bool,
    pub shown: bool,
}

pub(crate) struct Window {
    id: u64,
    options: Options,
    initial: String,
    pub result: Snapshot,
    reset_cursor: bool,
    activate: bool,
    was_focused: bool,
}

pub(crate) type Handle = Arc<Mutex<Window>>;

#[derive(Default)]
struct Registry {
    next: u64,
    windows: Vec<Handle>,
}

impl Registry {
    fn find(&self, key: &str) -> Option<Handle> {
        self.windows
            .iter()
            .find(|window| {
                let window = window.lock().unwrap();
                !window.result.closed && window.options.key == key
            })
            .cloned()
    }

    fn open(&mut self, options: Options, update: bool) -> Result<Handle, String> {
        check_text(&options.text)?;
        if !options.key.is_empty() {
            if let Some(handle) = self.find(&options.key) {
                let mut window = handle.lock().unwrap();
                if update {
                    // Quicker updates the document and caret in the existing window.
                    window.result.text.clone_from(&options.text);
                    window.result.selected.clear();
                    window.result.caret = options
                        .text
                        .chars()
                        .take(char_index(&options.text, options.caret))
                        .map(char::len_utf16)
                        .sum();
                    window.options.caret = options.caret;
                    window.reset_cursor = true;
                    drop(window);
                    return Ok(handle);
                }
                window.result.closed = true;
            }
        }
        self.windows.retain(|w| !w.lock().unwrap().result.closed);
        if self.windows.len() >= 32 {
            return Err("At most 32 text windows can be open".into());
        }
        self.next += 1;
        let window = Arc::new(Mutex::new(Window {
            id: self.next,
            initial: options.text.clone(),
            result: Snapshot {
                text: options.text.clone(),
                ..Default::default()
            },
            options,
            reset_cursor: true,
            activate: true,
            was_focused: false,
        }));
        self.windows.push(window.clone());
        Ok(window)
    }
}

#[derive(Clone)]
pub(crate) struct Host {
    registry: Arc<Mutex<Registry>>,
    context: Context,
}

static HOST: OnceLock<Host> = OnceLock::new();

pub(crate) fn host() -> Result<Host, String> {
    HOST.get()
        .cloned()
        .ok_or_else(|| "Text windows require the running native application".into())
}

impl Host {
    fn repaint(&self, id: u64) {
        self.context.request_repaint_of(ViewportId::ROOT);
        self.context
            .request_repaint_of(ViewportId::from_hash_of(("workflow-text", id)));
    }

    pub(crate) fn open(&self, options: Options, update: bool) -> Result<Handle, String> {
        let window = self.registry.lock().unwrap().open(options, update)?;
        self.repaint(window.lock().unwrap().id);
        Ok(window)
    }

    pub(crate) fn find(&self, key: &str) -> Result<Option<Handle>, String> {
        if key.is_empty() {
            return Err("A text window key is required".into());
        }
        Ok(self.registry.lock().unwrap().find(key))
    }

    pub(crate) fn close(&self, window: &Handle) {
        let mut window = window.lock().unwrap();
        window.result.closed = true;
        self.repaint(window.id);
    }

    pub(crate) fn append(&self, window: &Handle, text: &str) -> Result<(), String> {
        let mut window = window.lock().unwrap();
        if window.result.text.len().saturating_add(text.len()) > TEXT_LIMIT {
            return Err("Text window content exceeds 1 MiB".into());
        }
        window.result.text.push_str(text);
        self.repaint(window.id);
        Ok(())
    }

    pub(crate) fn activate(&self, window: &Handle) {
        let mut window = window.lock().unwrap();
        window.activate = true;
        self.repaint(window.id);
    }
}

pub(crate) fn check_text(text: &str) -> Result<(), String> {
    if text.len() > TEXT_LIMIT {
        Err("Text window content exceeds 1 MiB".into())
    } else {
        Ok(())
    }
}

pub(crate) fn status(window: &Handle) -> (bool, bool) {
    let window = window.lock().unwrap();
    (window.result.closed, window.result.shown)
}

pub(crate) fn snapshot(window: &Handle) -> Snapshot {
    window.lock().unwrap().result.clone()
}

pub(crate) fn render(ctx: &Context) {
    let host = HOST.get_or_init(|| Host {
        registry: Default::default(),
        context: ctx.clone(),
    });
    let windows = {
        let mut registry = host.registry.lock().unwrap();
        registry
            .windows
            .retain(|window| !window.lock().unwrap().result.closed);
        registry.windows.clone()
    };
    for handle in windows {
        let (id, builder) = {
            let window = handle.lock().unwrap();
            let options = &window.options;
            (
                ViewportId::from_hash_of(("workflow-text", window.id)),
                egui::ViewportBuilder::default()
                    .with_title(&options.title)
                    .with_inner_size(options.size)
                    .with_min_inner_size([240.0, 160.0])
                    .with_window_level(if options.top_most {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    }),
            )
        };
        ctx.show_viewport_deferred(id, builder, move |ctx, _| {
            let mut window = handle.lock().unwrap();
            window.ui(ctx);
        });
    }
}

fn char_index(text: &str, utf16: i64) -> usize {
    if utf16 < 0 {
        return text.chars().count();
    }
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= utf16 as usize
        })
        .count()
}

impl Window {
    fn ui(&mut self, ctx: &Context) {
        if self.result.closed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let first = !self.result.shown;
        if first {
            if let Some(command) = egui::ViewportCommand::center_on_screen(ctx) {
                ctx.send_viewport_cmd(command);
            }
        }
        if self.activate {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.activate = false;
        }
        let close = ctx.input(|i| {
            let focused = i.viewport().focused.unwrap_or(false);
            let blur = self.was_focused
                && !focused
                && self.options.close_on_blur
                && !self.options.top_most;
            self.was_focused |= focused;
            if let Some(rect) = i.viewport().outer_rect {
                let scale = i.pixels_per_point();
                self.result.position = format!(
                    "{},{},{},{}",
                    (rect.left() * scale).round(),
                    (rect.top() * scale).round(),
                    (rect.right() * scale).round(),
                    (rect.bottom() * scale).round()
                );
            }
            i.viewport().close_requested()
                || blur
                || (self.options.escape_close && i.key_pressed(egui::Key::Escape))
        });
        let mut select_all = false;
        if self.options.toolbar || !self.options.operations.is_empty() {
            egui::TopBottomPanel::top(egui::Id::new(("text-tools", self.id))).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if self.options.toolbar {
                        if ui.button("Copy all").clicked() {
                            ctx.copy_text(self.result.text.clone());
                        }
                        if ui.button("Select all").clicked() {
                            select_all = true;
                        }
                        if ui.button("Reset").clicked() {
                            self.result.text.clone_from(&self.initial);
                            self.reset_cursor = true;
                        }
                        ui.checkbox(&mut self.options.wrap, "Wrap");
                        if ui.button("Close").clicked() {
                            self.result.closed = true;
                        }
                    }
                    for (label, value) in &self.options.operations {
                        if ui.button(label).clicked() {
                            self.result.operation.clone_from(value);
                            self.result.closed = true;
                        }
                    }
                });
            });
        }
        let frame = egui::Frame::central_panel(&ctx.style()).fill(
            self.options
                .background
                .unwrap_or(ctx.style().visuals.extreme_bg_color),
        );
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            egui::ScrollArea::both()
                .id_salt(("text-scroll", self.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let gutter = if self.options.line_numbers { 48.0 } else { 0.0 };
                        let origin = ui.cursor().min;
                        if gutter > 0.0 {
                            ui.add_space(gutter);
                        }
                        let id = egui::Id::new(("text-editor", self.id));
                        if self.reset_cursor || select_all {
                            let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                            let cursor = egui::text::CCursor::new(char_index(
                                &self.result.text,
                                self.options.caret,
                            ));
                            state.cursor.set_char_range(Some(if select_all {
                                egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(self.result.text.chars().count()),
                                )
                            } else {
                                egui::text::CCursorRange::one(cursor)
                            }));
                            state.store(ctx, id);
                            self.reset_cursor = false;
                        }
                        let font = egui::FontId::monospace(self.options.font_size);
                        let before = self.result.text.clone();
                        let output = egui::TextEdit::multiline(&mut self.result.text)
                            .id(id)
                            .font(font.clone())
                            .frame(false)
                            .text_color(
                                self.options.foreground.unwrap_or(ui.visuals().text_color()),
                            )
                            .desired_width(if self.options.wrap {
                                ui.available_width()
                            } else {
                                f32::INFINITY
                            })
                            .desired_rows(20)
                            .char_limit(TEXT_LIMIT)
                            .show(ui);
                        if first || select_all {
                            output.response.request_focus();
                        }
                        if check_text(&self.result.text).is_err() {
                            self.result.text = before;
                        }
                        if let Some(range) = output.cursor_range {
                            let [start, end] = range.sorted_cursors();
                            self.result.selected = self
                                .result
                                .text
                                .chars()
                                .skip(start.index)
                                .take(end.index - start.index)
                                .collect();
                            self.result.caret = self
                                .result
                                .text
                                .chars()
                                .take(range.primary.index)
                                .map(char::len_utf16)
                                .sum();
                        }
                        if self.options.line_numbers {
                            let mut number = 1;
                            let mut starts_line = true;
                            for row in &output.galley.rows {
                                if starts_line {
                                    let position = egui::pos2(
                                        origin.x + gutter - 8.0,
                                        output.galley_pos.y + row.pos.y,
                                    );
                                    if ui
                                        .clip_rect()
                                        .intersects(egui::Rect::from_min_size(position, row.size))
                                    {
                                        ui.painter().text(
                                            position,
                                            egui::Align2::RIGHT_TOP,
                                            number.to_string(),
                                            font.clone(),
                                            ui.visuals().weak_text_color(),
                                        );
                                    }
                                }
                                starts_line = row.ends_with_newline;
                                if starts_line {
                                    number += 1;
                                }
                            }
                        }
                    });
                });
        });
        self.result.shown = true;
        if close || self.result.closed {
            self.result.closed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }
}

#[cfg(test)]
mod tests;
