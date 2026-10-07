//! Native list editing. The worker receives a copy only after confirmation.
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::action::{
    manage_list::{check_items, Options},
    ActionExecutionControl,
};
use egui::{Context, ViewportId};

type Handle = Arc<Mutex<Window>>;
type Outcome = Option<Vec<String>>;

struct Editor {
    index: usize,
    insert: bool,
    text: String,
    focus: bool,
}

struct Model {
    options: Options,
    items: Vec<String>,
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    editor: Option<Editor>,
    error: String,
}

impl Model {
    fn new(options: Options) -> Self {
        Self {
            items: options.items.clone(),
            options,
            selected: BTreeSet::new(),
            anchor: None,
            editor: None,
            error: String::new(),
        }
    }

    fn select(&mut self, index: usize, control: bool, shift: bool) {
        if shift {
            let anchor = self.anchor.unwrap_or(index);
            if !control {
                self.selected.clear();
            }
            self.selected.extend(anchor.min(index)..=anchor.max(index));
        } else {
            if !control {
                self.selected.clear();
            }
            if !self.selected.insert(index) {
                self.selected.remove(&index);
            }
            self.anchor = Some(index);
        }
    }

    fn edit(&mut self, insert: bool) {
        if self.editor.is_some()
            || (insert && !self.options.allow_add)
            || (!insert && !self.options.allow_edit)
        {
            return;
        }
        let index = if insert {
            self.selected.first().map_or(self.items.len(), |i| i + 1)
        } else if self.selected.len() == 1 {
            *self.selected.first().unwrap()
        } else {
            return;
        };
        self.editor = Some(Editor {
            index,
            insert,
            text: if insert {
                String::new()
            } else {
                self.items[index].clone()
            },
            focus: true,
        });
        self.error.clear();
    }

    fn apply_edit(&mut self) -> Result<(), String> {
        let edit = self.editor.as_ref().ok_or("No item is being edited")?;
        if edit.text.is_empty() {
            return Err("Enter an item value".into());
        }
        let mut changed = self.items.clone();
        if edit.insert {
            changed.insert(edit.index, edit.text.clone());
        } else {
            changed[edit.index] = edit.text.clone();
        }
        check_items(&changed)?;
        self.items = changed;
        self.selected = BTreeSet::from([edit.index]);
        self.anchor = Some(edit.index);
        self.editor = None;
        self.error.clear();
        Ok(())
    }

    fn delete(&mut self) {
        if !self.options.allow_delete || self.editor.is_some() {
            return;
        }
        for index in self.selected.iter().rev() {
            self.items.remove(*index);
        }
        self.selected.clear();
        self.anchor = None;
    }

    fn reset(&mut self) {
        self.items.clone_from(&self.options.items);
        self.selected.clear();
        self.anchor = None;
        self.error.clear();
    }

    fn sort(&mut self, descending: bool) {
        self.items.sort_by(|a, b| {
            let order = a.encode_utf16().cmp(b.encode_utf16());
            if descending {
                order.reverse()
            } else {
                order
            }
        });
        self.selected.clear();
        self.anchor = None;
    }

    fn move_to(&mut self, target: usize) {
        if self.selected.is_empty() || self.editor.is_some() {
            return;
        }
        let target = target.min(self.items.len());
        let destination = target - self.selected.range(..target).count();
        let moved: Vec<_> = self
            .selected
            .iter()
            .map(|i| self.items[*i].clone())
            .collect();
        for index in self.selected.iter().rev() {
            self.items.remove(*index);
        }
        let end = destination + moved.len();
        self.items.splice(destination..destination, moved);
        self.selected = (destination..end).collect();
        self.anchor = Some(destination);
    }
}

struct Window {
    id: u64,
    model: Model,
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
    ViewportId::from_hash_of(("workflow-list", id))
}

struct Open<'a> {
    host: &'a Host,
    handle: Handle,
}
impl Drop for Open<'_> {
    fn drop(&mut self) {
        let mut window = self.handle.lock().unwrap();
        window.result.get_or_insert(None);
        self.host.context.request_repaint_of(viewport(window.id));
        self.host.context.request_repaint_of(ViewportId::ROOT);
    }
}

pub(crate) fn show(options: Options, control: ActionExecutionControl) -> Result<Outcome, String> {
    let host = HOST
        .get()
        .ok_or("List editing requires the running native application")?;
    let handle = {
        let mut registry = host.registry.lock().unwrap();
        registry
            .windows
            .retain(|w| w.lock().unwrap().result.is_none());
        if registry.windows.len() >= 32 {
            return Err("At most 32 list windows can be open".into());
        }
        registry.next += 1;
        let window = Arc::new(Mutex::new(Window {
            id: registry.next,
            model: Model::new(options),
            result: None,
            shown: false,
        }));
        registry.windows.push(window.clone());
        window
    };
    let open = Open { host, handle };
    host.context.request_repaint_of(ViewportId::ROOT);
    loop {
        if control.is_cancelled() {
            return Err("Action cancelled".into());
        }
        if let Some(result) = &open.handle.lock().unwrap().result {
            return Ok(result.clone());
        }
        std::thread::sleep(Duration::from_millis(20));
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
            (
                viewport(w.id),
                egui::ViewportBuilder::default()
                    .with_title(&w.model.options.title)
                    .with_inner_size([w.model.options.width, 520.0])
                    .with_min_inner_size([200.0, 260.0]),
            )
        };
        ctx.show_viewport_deferred(id, builder, move |ctx, _| handle.lock().unwrap().ui(ctx));
    }
}

impl Window {
    fn ui(&mut self, ctx: &Context) {
        if self.result.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.shown {
            if let Some(command) = egui::ViewportCommand::center_on_screen(ctx) {
                ctx.send_viewport_cmd(command);
            }
            self.shown = true;
        }
        let close = ctx.input(|i| i.viewport().close_requested());
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if close {
            self.result = Some(None);
        } else if escape && self.model.editor.is_some() {
            self.model.editor = None;
            self.model.error.clear();
        } else if escape {
            self.result = Some(None);
        }
        let m = &mut self.model;
        if m.editor.is_none() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                m.selected = (0..m.items.len()).collect();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                m.delete();
            }
        }
        egui::TopBottomPanel::bottom("list-result").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(m.editor.is_none(), egui::Button::new("Done"))
                    .clicked()
                {
                    self.result = Some(Some(m.items.clone()));
                }
                if ui.button("Cancel").clicked() {
                    self.result = Some(None);
                }
                ui.label(format!("{} items", m.items.len()));
            });
        });
        if m.editor.is_some() {
            egui::TopBottomPanel::bottom("list-editor").show(ctx, |ui| {
                let edit = m.editor.as_mut().unwrap();
                ui.label(if edit.insert { "Add item" } else { "Edit item" });
                let before = edit.text.clone();
                let output = ui.add(
                    egui::TextEdit::singleline(&mut edit.text)
                        .desired_width(f32::INFINITY)
                        .char_limit(16 * 1024 * 1024),
                );
                if edit.focus {
                    output.request_focus();
                    edit.focus = false;
                }
                if edit.text.len() > 16 * 1024 * 1024 {
                    edit.text = before;
                }
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked()
                        || ctx.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        if let Err(error) = m.apply_edit() {
                            m.error = error;
                        }
                    }
                    if ui.button("Discard edit").clicked() {
                        m.editor = None;
                        m.error.clear();
                    }
                });
                if !m.error.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, &m.error);
                }
            });
        }
        egui::TopBottomPanel::top("list-tools").show(ctx, |ui| {
            ui.add_enabled_ui(m.editor.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(m.options.allow_add, egui::Button::new("Add"))
                        .clicked()
                    {
                        m.edit(true);
                    }
                    if ui
                        .add_enabled(
                            m.options.allow_edit && m.selected.len() == 1,
                            egui::Button::new("Edit"),
                        )
                        .clicked()
                    {
                        m.edit(false);
                    }
                    if ui
                        .add_enabled(
                            m.options.allow_delete && !m.selected.is_empty(),
                            egui::Button::new("Delete"),
                        )
                        .clicked()
                    {
                        m.delete();
                    }
                    if ui.button("A-Z").clicked() {
                        m.sort(false);
                    }
                    if ui.button("Z-A").clicked() {
                        m.sort(true);
                    }
                    if ui.button("Reset").clicked() {
                        m.reset();
                    }
                    if ui
                        .add_enabled(!m.selected.is_empty(), egui::Button::new("Up"))
                        .clicked()
                    {
                        let first = *m.selected.first().unwrap();
                        m.move_to(first.saturating_sub(1));
                    }
                    if ui
                        .add_enabled(!m.selected.is_empty(), egui::Button::new("Down"))
                        .clicked()
                    {
                        let last = *m.selected.last().unwrap();
                        m.move_to(last + 2);
                    }
                });
            });
            if !m.options.note.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("list-note")
                    .max_height(110.0)
                    .show(ui, |ui| {
                        ui.label(&m.options.note);
                    });
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(m.editor.is_none(), |ui| {
                let mut destination = None;
                egui::ScrollArea::vertical()
                    .id_salt("list-rows")
                    .auto_shrink([false, false])
                    .show_rows(ui, 24.0, m.items.len(), |ui, range| {
                        for index in range {
                            let text = m.items[index].replace(['\r', '\n'], " ↵ ");
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 24.0),
                                egui::Sense::click_and_drag(),
                            );
                            let visuals = ui
                                .style()
                                .interact_selectable(&response, m.selected.contains(&index));
                            ui.painter().rect_filled(rect, 2.0, visuals.bg_fill);
                            ui.painter().with_clip_rect(rect).text(
                                rect.left_center() + egui::vec2(4.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                text,
                                egui::FontId::proportional(14.0),
                                visuals.text_color(),
                            );
                            if response.clicked() {
                                response.request_focus();
                                let modifiers = ui.input(|i| i.modifiers);
                                m.select(index, modifiers.command, modifiers.shift);
                            }
                            if response.double_clicked() {
                                m.edit(false);
                            }
                            if response.drag_started() {
                                if !m.selected.contains(&index) {
                                    m.select(index, false, false);
                                }
                                response.dnd_set_drag_payload(self.id);
                            }
                            if response
                                .dnd_hover_payload::<u64>()
                                .is_some_and(|id| *id == self.id)
                            {
                                let below = ui.input(|i| {
                                    i.pointer
                                        .hover_pos()
                                        .is_some_and(|p| p.y >= rect.center().y)
                                });
                                let y = if below { rect.bottom() } else { rect.top() };
                                ui.painter().hline(
                                    rect.x_range(),
                                    y,
                                    (2.0, ui.visuals().selection.stroke.color),
                                );
                                if response.dnd_release_payload::<u64>().is_some() {
                                    destination = Some(index + usize::from(below));
                                }
                            }
                            response.on_hover_text(&m.items[index]);
                        }
                    });
                if let Some(target) = destination {
                    m.move_to(target);
                }
            });
        });
        if self.result.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }
}

#[cfg(test)]
mod tests;
