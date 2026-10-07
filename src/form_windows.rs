//! Form viewports. Validate submissions and run path pickers on worker threads.
use crate::action::{forms::Options, input_tools::Tool, ActionExecutionControl};
use egui::{Context, ViewportId};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

type Outcome = Result<Option<Vec<String>>, String>;
type Handle = Arc<Mutex<Window>>;
struct Window {
    id: u64,
    options: Options,
    values: Vec<String>,
    request: Option<(usize, Tool)>,
    submit: bool,
    busy: bool,
    error: String,
    focus: Option<usize>,
    result: Option<Outcome>,
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
    ViewportId::from_hash_of(("workflow-form", id))
}
impl Host {
    fn repaint(&self, id: u64) {
        self.context.request_repaint_of(ViewportId::ROOT);
        self.context.request_repaint_of(viewport(id));
    }
}
struct Open<'a> {
    host: &'a Host,
    handle: Handle,
}
impl Drop for Open<'_> {
    fn drop(&mut self) {
        let mut w = self.handle.lock().unwrap();
        w.result.get_or_insert(Ok(None));
        self.host.repaint(w.id);
    }
}
struct Picker {
    field: usize,
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

pub(crate) fn show(options: Options, control: ActionExecutionControl) -> Outcome {
    options.check_size()?;
    let host = HOST
        .get()
        .ok_or("Forms require the running native application")?;
    let handle = {
        let mut registry = host.registry.lock().unwrap();
        registry
            .windows
            .retain(|w| w.lock().unwrap().result.is_none());
        if registry.windows.len() >= 32 {
            return Err("At most 32 forms can be open".into());
        }
        registry.next += 1;
        let window = Arc::new(Mutex::new(Window {
            id: registry.next,
            values: options.fields.iter().map(|f| f.initial.clone()).collect(),
            focus: options
                .fields
                .iter()
                .position(|f| !f.readonly && f.method != 100),
            options,
            request: None,
            submit: false,
            busy: false,
            error: String::new(),
            result: None,
            shown: false,
        }));
        registry.windows.push(window.clone());
        window
    };
    let open = Open { host, handle };
    host.repaint(open.handle.lock().unwrap().id);
    let mut picker: Option<Picker> = None;
    loop {
        if control.is_cancelled() {
            return Err("Action cancelled".into());
        }
        {
            let mut w = open.handle.lock().unwrap();
            if let Some(result) = &w.result {
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
                    Ok(value) if value.len() <= 256 * 1024 => {
                        w.values[finished.field] = value;
                        w.focus = Some(finished.field);
                    }
                    Ok(_) => w.error = "Field value exceeds 256 KiB".into(),
                    Err(e) => w.error = e,
                }
                w.busy = false;
                host.repaint(w.id);
            }
            if let Some((field, tool)) = w.request.take() {
                let control = ActionExecutionControl::new();
                let child = control.clone();
                let value = w.values[field].clone();
                let thread = thread::Builder::new()
                    .name("form-path-picker".into())
                    .spawn(move || tool.choose(&value, &child))
                    .map_err(|e| format!("Cannot start file picker: {e}"))?;
                picker = Some(Picker {
                    field,
                    control,
                    thread: Some(thread),
                });
            }
            if w.submit {
                w.submit = false;
                // Release the UI lock while running regex validation.
                let options = w.options.clone();
                let values = w.values.clone();
                drop(w);
                let checked = options.values(&values);
                let mut w = open.handle.lock().unwrap();
                if w.result.is_none() {
                    match checked {
                        Ok(_) => w.result = Some(Ok(Some(values))),
                        Err(e) => w.error = e,
                    }
                    w.busy = false;
                    host.repaint(w.id);
                }
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
        let (id, builder) = {
            let w = handle.lock().unwrap();
            let natural = (w.options.fields.len() as f32 * 64.0 + 100.0).clamp(220.0, 640.0);
            let height = if w.options.height > 100.0 {
                natural.min(w.options.height)
            } else {
                natural
            };
            (
                viewport(w.id),
                egui::ViewportBuilder::default()
                    .with_title(&w.options.title)
                    .with_inner_size([w.options.width, height])
                    .with_min_inner_size([400.0, 160.0])
                    .with_window_level(if w.options.topmost {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    }),
            )
        };
        ctx.show_viewport_deferred(id, builder, move |ctx, _| handle.lock().unwrap().ui(ctx));
    }
}

// X11 can send a text event as well as an Alt shortcut key event.
fn consume_alt(ctx: &Context, key: egui::Key, letter: &str) -> bool {
    ctx.input_mut(|i| {
        let consumed = i.consume_key(egui::Modifiers::ALT, key);
        if consumed {
            i.events.retain(|event| !matches!(event, egui::Event::Text(text) if text.eq_ignore_ascii_case(letter)));
        }
        consumed
    })
}

impl Window {
    fn ui(&mut self, ctx: &Context) {
        if self.result.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.shown {
            if let Some(cmd) = egui::ViewportCommand::center_on_screen(ctx) {
                ctx.send_viewport_cmd(cmd);
            }
            self.shown = true;
        }
        let popup = egui::Popup::is_any_open(ctx);
        let close = ctx.input(|i| i.viewport().close_requested());
        let cancel = consume_alt(ctx, egui::Key::C, "c")
            || (!popup && ctx.input(|i| i.key_pressed(egui::Key::Escape)));
        let mut submit = consume_alt(ctx, egui::Key::S, "s");
        let mut reset = consume_alt(ctx, egui::Key::R, "r");
        egui::TopBottomPanel::bottom("form-controls").show(ctx, |ui| {
            if !self.error.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(70.0)
                    .id_salt("form-errors")
                    .show(ui, |ui| {
                        ui.colored_label(ui.visuals().error_fg_color, &self.error);
                    });
            }
            ui.horizontal(|ui| {
                submit |= ui
                    .add_enabled(!self.busy, egui::Button::new("Save"))
                    .clicked();
                if ui.button("Cancel").clicked() || cancel || close {
                    self.result = Some(Ok(None));
                }
                reset |= ui
                    .add_enabled(!self.busy, egui::Button::new("Reset"))
                    .clicked();
                if self.busy {
                    ui.spinner();
                }
            });
        });
        if reset && !self.busy {
            self.values = self
                .options
                .fields
                .iter()
                .map(|f| f.initial.clone())
                .collect();
            self.error.clear();
        }
        let mut multiline_focused = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("form-fields")
                .show(ui, |ui| {
                    ui.add_enabled_ui(!self.busy, |ui| {
                        for (index, field) in self.options.fields.iter().enumerate() {
                            ui.push_id(index, |ui| {
                                if field.method == 100 {
                                    ui.add_space(8.0);
                                    if field.label != "[]" {
                                        if !field.label.is_empty() {
                                            ui.strong(&field.label);
                                        }
                                        ui.separator();
                                    }
                                    return;
                                }
                                ui.horizontal_top(|ui| {
                                    ui.allocate_ui(
                                        egui::vec2(self.options.label_width, 22.0),
                                        |ui| {
                                            ui.set_min_width(self.options.label_width);
                                            ui.label(&field.label);
                                        },
                                    );
                                    ui.vertical(|ui| {
                                        let width = if field.width > 0.0 {
                                            field.width
                                        } else {
                                            self.options.input_width
                                        };
                                        let width = if width > 0.0 {
                                            width.min(ui.available_width())
                                        } else {
                                            ui.available_width()
                                        };
                                        ui.set_width(width.max(40.0));
                                        ui.add_enabled_ui(!field.readonly, |ui| {
                                            let value = &mut self.values[index];
                                            match field.method {
                                                3 => {
                                                    let title = field
                                                        .choices
                                                        .iter()
                                                        .find(|c| &c.value == value)
                                                        .map_or("", |c| c.title.as_str());
                                                    let response =
                                                        egui::ComboBox::from_id_salt("choice")
                                                            .selected_text(title)
                                                            .width(width)
                                                            .show_ui(ui, |ui| {
                                                                for choice in &field.choices {
                                                                    ui.selectable_value(
                                                                        value,
                                                                        choice.value.clone(),
                                                                        &choice.title,
                                                                    )
                                                                    .on_hover_text(&choice.help);
                                                                }
                                                            })
                                                            .response;
                                                    if self.focus == Some(index) {
                                                        response.request_focus();
                                                        self.focus = None;
                                                    }
                                                }
                                                6 => {
                                                    let mut checked =
                                                        matches!(value.as_str(), "true" | "1");
                                                    let response = ui.checkbox(&mut checked, "");
                                                    if response.changed() {
                                                        *value = checked.to_string();
                                                    }
                                                    if self.focus == Some(index) {
                                                        response.request_focus();
                                                        self.focus = None;
                                                    }
                                                }
                                                41 => {
                                                    ui.label(value.as_str());
                                                }
                                                _ => {
                                                    let before = value.clone();
                                                    let editor = if field.method == 2 {
                                                        egui::TextEdit::multiline(value)
                                                            .desired_rows(4)
                                                    } else {
                                                        egui::TextEdit::singleline(value)
                                                            .password(field.method == 10)
                                                    };
                                                    let response = ui.add(
                                                        editor.desired_width(width).char_limit(
                                                            if field.max_length > 0 {
                                                                field.max_length
                                                            } else {
                                                                256 * 1024
                                                            },
                                                        ),
                                                    );
                                                    if self.focus == Some(index) {
                                                        response.request_focus();
                                                        self.focus = None;
                                                    }
                                                    multiline_focused |=
                                                        field.method == 2 && response.has_focus();
                                                    if value.len() > 256 * 1024 {
                                                        *value = before;
                                                    }
                                                }
                                            }
                                            if !field.tools.is_empty() {
                                                ui.horizontal_wrapped(|ui| {
                                                    for tool in &field.tools {
                                                        if ui.button(tool.label()).clicked() {
                                                            self.request = Some((index, *tool));
                                                            self.busy = true;
                                                            self.error.clear();
                                                        }
                                                    }
                                                });
                                            }
                                        });
                                        if !field.help.is_empty() {
                                            ui.weak(&field.help);
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                            });
                        }
                    });
                    if !self.options.help.is_empty() {
                        ui.separator();
                        ui.label(&self.options.help);
                    }
                });
        });
        submit |= !self.options.disable_enter
            && !popup
            && !multiline_focused
            && ctx.input(|i| i.key_pressed(egui::Key::Enter));
        if submit && !self.busy && self.result.is_none() {
            self.submit = true;
            self.busy = true;
        }
        if self.result.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }
}
