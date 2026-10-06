use super::*;

pub(super) fn blank_action() -> Action {
    Action {
        name: String::new(),
        description: String::new(),
        icon: None,
        tags: Vec::new(),
        hotkey: None,
        kind: ActionKind::RunProgram {
            command: String::new(),
            args: Vec::new(),
            working_dir: None,
        },
    }
}

fn kinds() -> Vec<(&'static str, ActionKind)> {
    vec![
        (
            "Program",
            ActionKind::RunProgram {
                command: String::new(),
                args: vec![],
                working_dir: None,
            },
        ),
        (
            "Shell script",
            ActionKind::RunShell {
                script: String::new(),
                shell: "sh".into(),
            },
        ),
        ("URL", ActionKind::OpenUrl { url: String::new() }),
        (
            "File",
            ActionKind::OpenFile {
                path: String::new(),
            },
        ),
        (
            "Folder",
            ActionKind::OpenFolder {
                path: String::new(),
            },
        ),
        (
            "Copy text",
            ActionKind::CopyText {
                text: String::new(),
            },
        ),
        (
            "Search clipboard",
            ActionKind::SearchClipboardText {
                url_template: "https://www.google.com/search?q={query}".into(),
            },
        ),
        (
            "Open clipboard",
            ActionKind::OpenClipboardText {
                fallback_search_url: None,
            },
        ),
        (
            "Run clipboard",
            ActionKind::RunClipboardText { shell: "sh".into() },
        ),
        ("Group", ActionKind::Group { actions: vec![] }),
    ]
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn optional_field(ui: &mut egui::Ui, label: &str, value: &mut Option<String>) {
    let mut text = value.clone().unwrap_or_default();
    field(ui, label, &mut text);
    *value = (!text.is_empty()).then_some(text);
}

impl QuickerApp {
    pub(super) fn render_basic_editor(&mut self, ui: &mut egui::Ui) {
        let Some(action) = &mut self.basic_draft else {
            return;
        };
        field(ui, "Name", &mut action.name);
        field(ui, "Description", &mut action.description);
        optional_field(ui, "Icon (text or emoji)", &mut action.icon);

        let choices = kinds();
        let mut selected = choices
            .iter()
            .position(|(_, kind)| {
                std::mem::discriminant(kind) == std::mem::discriminant(&action.kind)
            })
            .unwrap_or(0);
        let previous = selected;
        egui::ComboBox::from_id_salt("basic_action_type")
            .selected_text(choices[selected].0)
            .show_ui(ui, |ui| {
                for (index, (label, _)) in choices.iter().enumerate() {
                    // Changing a populated group would discard its children.
                    let has_children = matches!(&action.kind, ActionKind::Group { actions } if !actions.is_empty());
                    ui.add_enabled_ui(!has_children || index == previous, |ui| {
                        ui.selectable_value(&mut selected, index, *label);
                    });
                }
            });
        if previous != selected {
            action.kind = choices[selected].1.clone();
        }
        ui.add_space(8.0);
        match &mut action.kind {
            ActionKind::RunProgram {
                command,
                args,
                working_dir,
            } => {
                field(ui, "Executable", command);
                optional_field(ui, "Working directory (optional)", working_dir);
                ui.label("Arguments (one value per row; spaces remain within that argument)");
                let mut remove = None;
                for (i, arg) in args.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(arg);
                            if ui.small_button("Remove").clicked() {
                                remove = Some(i);
                            }
                        })
                    });
                }
                if let Some(i) = remove {
                    args.remove(i);
                }
                if ui.button("Add argument").clicked() {
                    args.push(String::new());
                }
            }
            ActionKind::RunShell { script, shell } => {
                field(ui, "Shell", shell);
                ui.label("Script");
                ui.add(
                    egui::TextEdit::multiline(script)
                        .code_editor()
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                );
            }
            ActionKind::OpenUrl { url } => field(ui, "URL", url),
            ActionKind::OpenFile { path } | ActionKind::OpenFolder { path } => {
                field(ui, "Path", path)
            }
            ActionKind::CopyText { text } => {
                ui.label("Text");
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );
            }
            ActionKind::SearchClipboardText { url_template } => field(
                ui,
                "Search URL (use {query} for clipboard text)",
                url_template,
            ),
            ActionKind::OpenClipboardText {
                fallback_search_url,
            } => optional_field(
                ui,
                "Fallback search URL (optional, use {query})",
                fallback_search_url,
            ),
            ActionKind::RunClipboardText { shell } => field(ui, "Shell", shell),
            ActionKind::Group { actions } => {
                ui.label(format!(
                    "{} child actions. Open this group in the panel to add or edit its contents.",
                    actions.len()
                ));
            }
            ActionKind::PluginPipeline { .. } => {}
        }
        ui.add_space(12.0);
        ui.collapsing("Action JSON import / export", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Export JSON").clicked() {
                    self.edit_field1 = serde_json::to_string_pretty(action).unwrap_or_default();
                }
                if ui.button("Copy JSON").clicked() {
                    ui.ctx().copy_text(self.edit_field1.clone());
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.edit_field1)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
        });
        if ui.button("Import Action JSON").clicked() {
            match serde_json::from_str::<Action>(&self.edit_field1) {
                Ok(imported) => {
                    if let ActionKind::PluginPipeline { plugin } = &imported.kind {
                        self.edit_field1 = plugin.quicker_json.clone();
                        self.plugin_editor_mode = PluginEditorMode::RawJson {
                            reason: "Imported plugin document".into(),
                        };
                        self.basic_draft = None;
                    } else {
                        self.basic_draft = Some(imported);
                    }
                }
                Err(err) => self.show_toast(format!("Invalid action JSON: {err}"), true),
            }
        }
    }
}
