//! Text input with native path pickers. Workers wait. The UI thread stays available.
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use egui::{Context, ViewportId};

use crate::action::{input_tools::Tool, ActionExecutionControl};

const LIMIT: usize = 1024 * 1024;
type Handle = Arc<Mutex<Window>>;

struct Window {
    id: u64,
    prompt: String,
    text: String,
    multiline: bool,
    tools: Vec<Tool>,
    caret: Option<usize>,
    request: Option<Tool>,
    busy: bool,
    error: String,
    result: Option<Result<String, String>>,
    shown: bool,
}

#[derive(Default)]
struct Registry {
    next: u64,
    windows: Vec<Handle>,
}

struct Host {
    context: Context,
    registry: Mutex<Registry>,
}

static HOST: OnceLock<Host> = OnceLock::new();

fn viewport(id: u64) -> ViewportId {
    ViewportId::from_hash_of(("workflow-input", id))
}

impl Host {
    fn repaint(&self, id: u64) {
        self.context.request_repaint_of(ViewportId::ROOT);
        self.context.request_repaint_of(viewport(id));
    }
}

struct OpenWindow<'a> {
    host: &'a Host,
    window: Handle,
}

impl Drop for OpenWindow<'_> {
    fn drop(&mut self) {
        let mut window = self.window.lock().unwrap();
        window
            .result
            .get_or_insert_with(|| Err("Input cancelled".into()));
        self.host.repaint(window.id);
    }
}

// Cancelling or closing the input must also stop an active native picker.
struct Picker {
    control: ActionExecutionControl,
    thread: Option<thread::JoinHandle<Result<String, String>>>,
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.control.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn prompt(
    prompt: &str,
    initial: &str,
    multiline: bool,
    tools: &[Tool],
    control: ActionExecutionControl,
) -> Result<String, String> {
    if prompt.len() > LIMIT || initial.len() > LIMIT {
        return Err("Input content exceeds 1 MiB".into());
    }
    let host = HOST
        .get()
        .ok_or("Input text tools require the running native application")?;
    let window = {
        let mut registry = host.registry.lock().unwrap();
        registry
            .windows
            .retain(|w| w.lock().unwrap().result.is_none());
        if registry.windows.len() >= 32 {
            return Err("At most 32 input windows can be open".into());
        }
        registry.next += 1;
        let window = Arc::new(Mutex::new(Window {
            id: registry.next,
            prompt: prompt.into(),
            text: initial.into(),
            multiline,
            tools: tools.to_vec(),
            caret: None,
            request: None,
            busy: false,
            error: String::new(),
            result: None,
            shown: false,
        }));
        registry.windows.push(window.clone());
        window
    };
    let open = OpenWindow { host, window };
    host.repaint(open.window.lock().unwrap().id);
    let mut picker: Option<Picker> = None;
    loop {
        if control.is_cancelled() {
            return Err("Action cancelled".into());
        }
        {
            let mut window = open.window.lock().unwrap();
            if let Some(result) = &window.result {
                return result.clone();
            }
            if picker
                .as_ref()
                .is_some_and(|p| p.thread.as_ref().unwrap().is_finished())
            {
                let mut finished = picker.take().unwrap();
                let result = finished
                    .thread
                    .take()
                    .unwrap()
                    .join()
                    .unwrap_or_else(|_| Err("File picker failed".into()));
                match result {
                    Ok(text) => {
                        if text.len() <= LIMIT {
                            window.caret = Some(text.chars().count());
                            window.text = text;
                        } else {
                            window.error = "Input content exceeds 1 MiB".into();
                        }
                    }
                    Err(error) => window.error = error,
                }
                window.busy = false;
                host.repaint(window.id);
            }
            if let Some(tool) = window.request.take() {
                let control = ActionExecutionControl::new();
                let child_control = control.clone();
                let text = window.text.clone();
                let thread = thread::Builder::new()
                    .name("input-path-picker".into())
                    .spawn(move || tool.choose(&text, &child_control))
                    .map_err(|e| format!("Cannot start file picker: {e}"))?;
                picker = Some(Picker {
                    control,
                    thread: Some(thread),
                });
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn render(ctx: &Context) {
    let host = HOST.get_or_init(|| Host {
        context: ctx.clone(),
        registry: Mutex::default(),
    });
    let windows = {
        let mut registry = host.registry.lock().unwrap();
        registry
            .windows
            .retain(|w| w.lock().unwrap().result.is_none());
        registry.windows.clone()
    };
    for handle in windows {
        let (id, multiline) = {
            let window = handle.lock().unwrap();
            (window.id, window.multiline)
        };
        let builder = egui::ViewportBuilder::default()
            .with_title("Quicker input")
            .with_inner_size([560.0, if multiline { 340.0 } else { 200.0 }])
            .with_min_inner_size([320.0, 180.0]);
        ctx.show_viewport_deferred(viewport(id), builder, move |ctx, _| {
            handle.lock().unwrap().ui(ctx);
        });
    }
}

impl Window {
    fn ui(&mut self, ctx: &Context) {
        if self.result.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let first = !self.shown;
        if first {
            if let Some(command) = egui::ViewportCommand::center_on_screen(ctx) {
                ctx.send_viewport_cmd(command);
            }
        }
        let cancel =
            ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape));
        let submit = !self.busy
            && ctx.input(|i| {
                (i.key_pressed(egui::Key::Enter) && (!self.multiline || i.modifiers.ctrl))
                    || (i.modifiers.alt && i.key_pressed(egui::Key::S))
            });
        egui::TopBottomPanel::bottom("input-buttons").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("OK"))
                    .clicked()
                    || submit
                {
                    self.result = Some(Ok(self.text.clone()));
                }
                if ui.button("Cancel").clicked() || cancel {
                    self.result = Some(Err("Input cancelled".into()));
                }
                if self.busy {
                    ui.spinner();
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(&self.prompt);
                ui.add_enabled_ui(!self.busy, |ui| {
                    let id = egui::Id::new(("input-editor", self.id));
                    let focus = first || self.caret.is_some();
                    if focus {
                        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(if let Some(caret) = self.caret.take() {
                                egui::text::CCursorRange::one(egui::text::CCursor::new(caret))
                            } else {
                                egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(self.text.chars().count()),
                                )
                            }));
                        state.store(ctx, id);
                    }
                    let before = self.text.clone();
                    let editor = if self.multiline {
                        egui::TextEdit::multiline(&mut self.text)
                    } else {
                        egui::TextEdit::singleline(&mut self.text)
                    };
                    let output = editor
                        .id(id)
                        .desired_width(f32::INFINITY)
                        .desired_rows(if self.multiline { 8 } else { 1 })
                        .char_limit(LIMIT)
                        .show(ui);
                    if focus {
                        output.response.request_focus();
                    }
                    if self.text.len() > LIMIT {
                        self.text = before;
                    }
                    ui.horizontal_wrapped(|ui| {
                        for tool in &self.tools {
                            if ui.button(tool.label()).clicked() {
                                self.request = Some(*tool);
                                self.busy = true;
                                self.error.clear();
                            }
                        }
                    });
                });
                if !self.error.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, &self.error);
                }
            });
        });
        self.shown = true;
        if self.result.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }
}
