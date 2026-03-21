use super::*;

impl QuickerApp {
    fn plugin_step_kind_labels() -> &'static [&'static str] {
        &[
            "Open URL",
            "Delay",
            "If",
            "State Read",
            "State Write",
            "Message Box",
            "Select Folder",
            "User Input",
            "Download File",
            "Read File",
            "Image Info",
            "Image To Base64",
            "Delete File",
            "Key Input",
            "Get Clipboard",
            "Write Clipboard",
            "Regex Extract",
            "String Process",
            "Split String",
            "Assign",
            "Replace Text",
            "Format String",
            "Notify",
            "Output Text",
        ]
    }

    fn key_macro_step_kind_labels() -> [&'static str; 3] {
        ["Send Keys", "Type Text", "Delay"]
    }

    fn make_key_macro_step(kind_idx: usize) -> LowCodeKeyMacroStep {
        match kind_idx {
            0 => LowCodeKeyMacroStep::SendKeys {
                modifiers: "ctrl".into(),
                key: "C".into(),
            },
            1 => LowCodeKeyMacroStep::TypeText {
                text: "example".into(),
            },
            2 => LowCodeKeyMacroStep::Delay { delay_ms: 100 },
            _ => LowCodeKeyMacroStep::SendKeys {
                modifiers: "ctrl".into(),
                key: "C".into(),
            },
        }
    }

    fn make_plugin_step(kind_idx: usize) -> LowCodePluginStep {
        match kind_idx {
            0 => LowCodePluginStep::OpenUrl {
                url: "https://example.com".into(),
            },
            1 => LowCodePluginStep::Delay { delay_ms: 100 },
            2 => LowCodePluginStep::SimpleIf {
                condition: "$is_true".into(),
                if_steps: vec![LowCodePluginStep::Notify {
                    message: "If branch".into(),
                }],
                else_steps: Vec::new(),
            },
            3 => LowCodePluginStep::StateStorageRead {
                key: "path".into(),
                default_value: String::new(),
                output_value: "path".into(),
                output_is_empty: "is_path_empty".into(),
            },
            4 => LowCodePluginStep::StateStorageWrite {
                key: "path".into(),
                value: "$path".into(),
            },
            5 => LowCodePluginStep::MsgBox {
                title: "Notice".into(),
                message: "Hello".into(),
            },
            6 => LowCodePluginStep::SelectFolder {
                prompt: "Select a folder".into(),
                output: "path".into(),
            },
            7 => LowCodePluginStep::UserInput {
                prompt: "Enter text".into(),
                default_value: String::new(),
                multiline: true,
                output: "text".into(),
            },
            8 => LowCodePluginStep::DownloadFile {
                url: "https://example.com/file.png".into(),
                save_path: "$path".into(),
                save_name: "file.png".into(),
                output_success: "ok".into(),
            },
            9 => LowCodePluginStep::ReadFileImage {
                path: "$path\\file.png".into(),
                output: "img".into(),
            },
            10 => LowCodePluginStep::ImageInfo {
                source: "$img".into(),
                width_output: "width".into(),
                height_output: "height".into(),
            },
            11 => LowCodePluginStep::ImageToBase64 {
                source: "$img".into(),
                output: "base64".into(),
            },
            12 => LowCodePluginStep::FileDelete {
                path: "$path\\file.png".into(),
                disabled: false,
            },
            13 => LowCodePluginStep::KeyInput {
                modifiers: "ctrl".into(),
                key: "V".into(),
            },
            14 => LowCodePluginStep::GetClipboard {
                format: LowCodeClipboardFormat::Text,
                output: "text".into(),
            },
            15 => LowCodePluginStep::WriteClipboard {
                clipboard_type: LowCodeWriteClipboardKind::Auto,
                source: "$text".into(),
                alt_text: String::new(),
            },
            16 => LowCodePluginStep::RegexExtract {
                input: "$text".into(),
                pattern: String::new(),
                output: "match".into(),
            },
            17 => LowCodePluginStep::StringProcess {
                input: "$text".into(),
                method: LowCodeStringProcessMethod::ToLower,
                output: "output".into(),
            },
            18 => LowCodePluginStep::SplitString {
                input: "$text".into(),
                separator: "\\r\\n".into(),
                output: "parts".into(),
            },
            19 => LowCodePluginStep::Assign {
                expression: "$={parts}[0]".into(),
                output: "first_part".into(),
            },
            20 => LowCodePluginStep::StrReplace {
                input: "$text".into(),
                pattern: String::new(),
                replacement: String::new(),
                use_regex: true,
                output: "output".into(),
            },
            21 => LowCodePluginStep::FormatString {
                template: "{0}".into(),
                p0: "$text".into(),
                p1: String::new(),
                p2: String::new(),
                p3: String::new(),
                p4: String::new(),
                output: "output".into(),
            },
            22 => LowCodePluginStep::Notify {
                message: "Done".into(),
            },
            23 => LowCodePluginStep::OutputText {
                content: "$text".into(),
                append_return: false,
            },
            _ => LowCodePluginStep::Notify {
                message: "Done".into(),
            },
        }
    }

    fn render_key_macro_step_card(
        ui: &mut egui::Ui,
        index: usize,
        step: &mut LowCodeKeyMacroStep,
    ) -> bool {
        let mut remove = false;

        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⋮⋮").size(16.0).weak());
                    ui.label(
                        egui::RichText::new(format!("Step {}: {}", index + 1, step.label()))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            remove = true;
                        }
                    });
                });
                ui.add_space(6.0);

                match step {
                    LowCodeKeyMacroStep::SendKeys { modifiers, key } => {
                        ui.label("Modifiers (ctrl+shift style):");
                        ui.text_edit_singleline(modifiers);
                        ui.label("Key:");
                        ui.text_edit_singleline(key);
                    }
                    LowCodeKeyMacroStep::TypeText { text } => {
                        ui.label("Text:");
                        ui.text_edit_singleline(text);
                    }
                    LowCodeKeyMacroStep::Delay { delay_ms } => {
                        ui.label("Delay (ms):");
                        ui.add(egui::DragValue::new(delay_ms).range(0..=60_000).speed(10));
                    }
                }
            });

        remove
    }

    fn render_nested_plugin_step_list(
        ui: &mut egui::Ui,
        id_scope: &str,
        steps: &mut Vec<LowCodePluginStep>,
    ) {
        let mut add_step_idx = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Branch Steps").weak().small());
            ui.menu_button("Add Step", |ui| {
                for (idx, label) in Self::plugin_step_kind_labels().iter().enumerate() {
                    if ui.button(*label).clicked() {
                        add_step_idx = Some(idx);
                        ui.close();
                    }
                }
            });
        });
        ui.add_space(4.0);

        if let Some(idx) = add_step_idx {
            steps.push(Self::make_plugin_step(idx));
        }

        let mut remove_idx = None;
        let mut move_request = None;
        let drop_frame = egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 4))
            .stroke(egui::Stroke::new(
                1.0,
                ui.visuals().widgets.inactive.bg_stroke.color,
            ));

        if steps.is_empty() {
            ui.label(egui::RichText::new("No steps in this branch.").weak());
            return;
        }

        for insert_idx in 0..=steps.len() {
            let (_, dropped) = ui.dnd_drop_zone::<StepDragPayload, _>(drop_frame, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Drop step here").weak().small());
                });
            });
            if let Some(payload) = dropped {
                if payload.scope == id_scope {
                    move_request = Some((payload.from, insert_idx));
                }
            }

            if insert_idx == steps.len() {
                break;
            }

            let response = Self::render_plugin_step_card(
                ui,
                &format!("{id_scope}_{insert_idx}"),
                Some(id_scope),
                insert_idx,
                &mut steps[insert_idx],
                false,
            );
            if response.remove {
                remove_idx = Some(insert_idx);
            }
            ui.add_space(4.0);
        }

        if let Some((from, mut to)) = move_request {
            if from < steps.len() {
                if from < to {
                    to -= 1;
                }
                if from != to && to <= steps.len() {
                    let step = steps.remove(from);
                    steps.insert(to, step);
                }
            }
        }
        if let Some(idx) = remove_idx {
            steps.remove(idx);
        }
    }

    fn render_plugin_step_card(
        ui: &mut egui::Ui,
        id_scope: &str,
        drag_scope: Option<&str>,
        index: usize,
        step: &mut LowCodePluginStep,
        _show_reorder_buttons: bool,
    ) -> StepCardAction {
        let mut action = StepCardAction::default();

        ui.push_id(id_scope, |ui| {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(scope) = drag_scope {
                            let _ = ui.dnd_drag_source(
                                egui::Id::new((scope, index, "handle")),
                                StepDragPayload {
                                    scope: scope.to_string(),
                                    from: index,
                                },
                                |ui| {
                                    ui.label(egui::RichText::new("⋮⋮").size(16.0).weak());
                                },
                            );
                        } else {
                            ui.label(egui::RichText::new("⋮⋮").size(16.0).weak());
                        }
                        ui.label(
                            egui::RichText::new(format!("Step {}: {}", index + 1, step.label()))
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Delete").clicked() {
                                action.remove = true;
                            }
                        });
                    });
                    ui.add_space(6.0);

                    match step {
                        LowCodePluginStep::OpenUrl { url } => {
                            ui.label("URL or $variable:");
                            ui.text_edit_singleline(url);
                        }
                        LowCodePluginStep::Delay { delay_ms } => {
                            ui.label("Delay (ms):");
                            ui.add(egui::DragValue::new(delay_ms).range(0..=60_000).speed(10));
                        }
                        LowCodePluginStep::SimpleIf {
                            condition,
                            if_steps,
                            else_steps,
                        } => {
                            ui.label("Condition value or $variable:");
                            ui.text_edit_singleline(condition);
                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.label(egui::RichText::new("If Branch").strong());
                                Self::render_nested_plugin_step_list(
                                    ui,
                                    &format!("{id_scope}_if"),
                                    if_steps,
                                );
                            });
                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.label(egui::RichText::new("Else Branch").strong());
                                Self::render_nested_plugin_step_list(
                                    ui,
                                    &format!("{id_scope}_else"),
                                    else_steps,
                                );
                            });
                        }
                        LowCodePluginStep::StateStorageRead {
                            key,
                            default_value,
                            output_value,
                            output_is_empty,
                        } => {
                            ui.label("State key:");
                            ui.text_edit_singleline(key);
                            ui.label("Default value:");
                            ui.text_edit_singleline(default_value);
                            ui.label("Output value variable:");
                            ui.text_edit_singleline(output_value);
                            ui.label("Output empty-flag variable:");
                            ui.text_edit_singleline(output_is_empty);
                        }
                        LowCodePluginStep::StateStorageWrite { key, value } => {
                            ui.label("State key:");
                            ui.text_edit_singleline(key);
                            ui.label("Value or $variable:");
                            ui.text_edit_singleline(value);
                        }
                        LowCodePluginStep::MsgBox { title, message } => {
                            ui.label("Title:");
                            ui.text_edit_singleline(title);
                            ui.label("Message:");
                            ui.text_edit_singleline(message);
                        }
                        LowCodePluginStep::SelectFolder { prompt, output } => {
                            ui.label("Prompt:");
                            ui.text_edit_singleline(prompt);
                            ui.label("Output path variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::UserInput {
                            prompt,
                            default_value,
                            multiline,
                            output,
                        } => {
                            ui.label("Prompt:");
                            ui.text_edit_singleline(prompt);
                            ui.label("Default value:");
                            ui.text_edit_singleline(default_value);
                            ui.checkbox(multiline, "Multiline");
                            ui.label("Output text variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::DownloadFile {
                            url,
                            save_path,
                            save_name,
                            output_success,
                        } => {
                            ui.label("URL or $variable:");
                            ui.text_edit_singleline(url);
                            ui.label("Save folder or $variable:");
                            ui.text_edit_singleline(save_path);
                            ui.label("File name:");
                            ui.text_edit_singleline(save_name);
                            ui.label("Success flag variable:");
                            ui.text_edit_singleline(output_success);
                        }
                        LowCodePluginStep::ReadFileImage { path, output } => {
                            ui.label("Image path or expression:");
                            ui.text_edit_singleline(path);
                            ui.label("Output image variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::ImageInfo {
                            source,
                            width_output,
                            height_output,
                        } => {
                            ui.label("Source image variable:");
                            ui.text_edit_singleline(source);
                            ui.label("Width output variable:");
                            ui.text_edit_singleline(width_output);
                            ui.label("Height output variable:");
                            ui.text_edit_singleline(height_output);
                        }
                        LowCodePluginStep::ImageToBase64 { source, output } => {
                            ui.label("Source image variable:");
                            ui.text_edit_singleline(source);
                            ui.label("Output base64 variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::FileDelete { path, disabled } => {
                            ui.label("File path or expression:");
                            ui.text_edit_singleline(path);
                            ui.checkbox(disabled, "Disabled");
                        }
                        LowCodePluginStep::KeyInput { modifiers, key } => {
                            ui.label("Modifiers (ctrl+shift style):");
                            ui.text_edit_singleline(modifiers);
                            ui.label("Key:");
                            ui.text_edit_singleline(key);
                        }
                        LowCodePluginStep::GetClipboard { format, output } => {
                            ui.horizontal(|ui| {
                                ui.label("Format:");
                                egui::ComboBox::from_id_salt(("clip_format", index))
                                    .selected_text(match format {
                                        LowCodeClipboardFormat::Text => "Text",
                                        LowCodeClipboardFormat::Html => "HTML",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            format,
                                            LowCodeClipboardFormat::Text,
                                            "Text",
                                        );
                                        ui.selectable_value(
                                            format,
                                            LowCodeClipboardFormat::Html,
                                            "HTML",
                                        );
                                    });
                            });
                            ui.label("Output variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::WriteClipboard {
                            clipboard_type,
                            source,
                            alt_text,
                        } => {
                            ui.horizontal(|ui| {
                                ui.label("Clipboard type:");
                                egui::ComboBox::from_id_salt(("write_clip_kind", index))
                                    .selected_text(match clipboard_type {
                                        LowCodeWriteClipboardKind::Auto => "Auto",
                                        LowCodeWriteClipboardKind::Text => "Text",
                                        LowCodeWriteClipboardKind::Html => "HTML",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            clipboard_type,
                                            LowCodeWriteClipboardKind::Auto,
                                            "Auto",
                                        );
                                        ui.selectable_value(
                                            clipboard_type,
                                            LowCodeWriteClipboardKind::Text,
                                            "Text",
                                        );
                                        ui.selectable_value(
                                            clipboard_type,
                                            LowCodeWriteClipboardKind::Html,
                                            "HTML",
                                        );
                                    });
                            });
                            ui.label("Source text or $variable:");
                            ui.text_edit_singleline(source);
                            if matches!(clipboard_type, LowCodeWriteClipboardKind::Html) {
                                ui.label("Plain-text fallback:");
                                ui.text_edit_singleline(alt_text);
                            }
                        }
                        LowCodePluginStep::RegexExtract {
                            input,
                            pattern,
                            output,
                        } => {
                            ui.label("Input text or $variable:");
                            ui.text_edit_singleline(input);
                            ui.label("Regex pattern:");
                            ui.text_edit_singleline(pattern);
                            ui.label("Output variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::StringProcess {
                            input,
                            method,
                            output,
                        } => {
                            ui.label("Input text or $variable:");
                            ui.text_edit_singleline(input);
                            ui.horizontal(|ui| {
                                ui.label("Method:");
                                egui::ComboBox::from_id_salt(("string_process", index))
                                    .selected_text(match method {
                                        LowCodeStringProcessMethod::ToLower => "toLower",
                                        LowCodeStringProcessMethod::UrlEncode => "urlEncode",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            method,
                                            LowCodeStringProcessMethod::ToLower,
                                            "toLower",
                                        );
                                        ui.selectable_value(
                                            method,
                                            LowCodeStringProcessMethod::UrlEncode,
                                            "urlEncode",
                                        );
                                    });
                            });
                            ui.label("Output variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::SplitString {
                            input,
                            separator,
                            output,
                        } => {
                            ui.label("Input text or $variable:");
                            ui.text_edit_singleline(input);
                            ui.label("Separator (supports escapes like \\r\\n):");
                            ui.text_edit_singleline(separator);
                            ui.label("Output array variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::Assign { expression, output } => {
                            ui.label("Expression or source value:");
                            ui.text_edit_singleline(expression);
                            ui.label("Output variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::StrReplace {
                            input,
                            pattern,
                            replacement,
                            use_regex,
                            output,
                        } => {
                            ui.label("Input text or $variable:");
                            ui.text_edit_singleline(input);
                            ui.label("Pattern / old text:");
                            ui.text_edit_singleline(pattern);
                            ui.label("Replacement:");
                            ui.text_edit_singleline(replacement);
                            ui.checkbox(use_regex, "Use regex");
                            ui.label("Output variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::FormatString {
                            template,
                            p0,
                            p1,
                            p2,
                            p3,
                            p4,
                            output,
                        } => {
                            ui.label("Template:");
                            ui.text_edit_singleline(template);
                            for (label, value) in
                                [("P0", p0), ("P1", p1), ("P2", p2), ("P3", p3), ("P4", p4)]
                            {
                                ui.label(format!("{label} value or $variable:"));
                                ui.text_edit_singleline(value);
                            }
                            ui.label("Output variable:");
                            ui.text_edit_singleline(output);
                        }
                        LowCodePluginStep::Notify { message } => {
                            ui.label("Message:");
                            ui.text_edit_singleline(message);
                        }
                        LowCodePluginStep::OutputText {
                            content,
                            append_return,
                        } => {
                            ui.label("Content or $variable:");
                            ui.text_edit_singleline(content);
                            ui.checkbox(append_return, "Append Return");
                        }
                    }
                });
        });

        action
    }

    fn render_plugin_metadata_fields(ui: &mut egui::Ui, draft: &mut LowCodePluginDraft) {
        ui.label("Title:");
        ui.text_edit_singleline(&mut draft.title);
        ui.label("Description:");
        ui.text_edit_singleline(&mut draft.description);
        ui.label("Icon:");
        let mut icon_text = draft.icon.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut icon_text).changed() {
            draft.icon = if icon_text.trim().is_empty() {
                None
            } else {
                Some(icon_text)
            };
        }
    }

    fn plugin_flow_variable_names(steps: &[LowCodePluginStep]) -> Vec<String> {
        let mut names = BTreeSet::new();
        for step in steps {
            match step {
                LowCodePluginStep::SimpleIf {
                    if_steps,
                    else_steps,
                    ..
                } => {
                    names.extend(Self::plugin_flow_variable_names(if_steps));
                    names.extend(Self::plugin_flow_variable_names(else_steps));
                }
                LowCodePluginStep::StateStorageRead {
                    output_value,
                    output_is_empty,
                    ..
                } => {
                    for output in [output_value, output_is_empty] {
                        let trimmed = output.trim();
                        if !trimmed.is_empty() {
                            names.insert(trimmed.to_string());
                        }
                    }
                }
                LowCodePluginStep::SelectFolder { output, .. }
                | LowCodePluginStep::UserInput { output, .. }
                | LowCodePluginStep::DownloadFile {
                    output_success: output,
                    ..
                }
                | LowCodePluginStep::ReadFileImage { output, .. }
                | LowCodePluginStep::ImageToBase64 { output, .. } => {
                    let trimmed = output.trim();
                    if !trimmed.is_empty() {
                        names.insert(trimmed.to_string());
                    }
                }
                LowCodePluginStep::ImageInfo {
                    width_output,
                    height_output,
                    ..
                } => {
                    for output in [width_output, height_output] {
                        let trimmed = output.trim();
                        if !trimmed.is_empty() {
                            names.insert(trimmed.to_string());
                        }
                    }
                }
                LowCodePluginStep::GetClipboard { output, .. }
                | LowCodePluginStep::RegexExtract { output, .. }
                | LowCodePluginStep::StringProcess { output, .. }
                | LowCodePluginStep::SplitString { output, .. }
                | LowCodePluginStep::Assign { output, .. }
                | LowCodePluginStep::StrReplace { output, .. }
                | LowCodePluginStep::FormatString { output, .. } => {
                    let trimmed = output.trim();
                    if !trimmed.is_empty() {
                        names.insert(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }
        names.into_iter().collect()
    }

    fn render_plugin_flow_builder(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Plugin Flow").strong());
        ui.label(
            egui::RichText::new(
                "Reference layout: left palette, center flow, right properties. Drag cards in the middle column to reorder them.",
            )
            .weak()
            .small(),
        );
        ui.add_space(8.0);

        ui.columns(3, |columns| {
            columns[0].group(|ui| {
                ui.label(egui::RichText::new("Step Palette").strong());
                ui.label(egui::RichText::new("Click to add a step.").weak().small());
                ui.add_space(6.0);
                for (idx, label) in Self::plugin_step_kind_labels().iter().enumerate() {
                    if ui
                        .add_sized([ui.available_width(), 28.0], egui::Button::new(*label))
                        .clicked()
                    {
                        self.plugin_draft.steps.push(Self::make_plugin_step(idx));
                    }
                }
            });

            columns[1].group(|ui| {
                ui.label(egui::RichText::new("Main Steps").strong());
                ui.label(
                    egui::RichText::new("This is the executable order of the flow.")
                        .weak()
                        .small(),
                );
                ui.add_space(6.0);

                let mut remove_idx = None;
                let mut move_request = None;
                let drop_frame = egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(egui::Stroke::new(
                        1.0,
                        ui.visuals().widgets.inactive.bg_stroke.color,
                    ));

                if self.plugin_draft.steps.is_empty() {
                    ui.label(
                        egui::RichText::new("No steps yet. Add one from the left palette.").weak(),
                    );
                } else {
                    for insert_idx in 0..=self.plugin_draft.steps.len() {
                        let (_, dropped) =
                            ui.dnd_drop_zone::<StepDragPayload, _>(drop_frame, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new("Drop step here").weak().small());
                                });
                            });
                        if let Some(payload) = dropped {
                            if payload.scope == "root" {
                                move_request = Some((payload.from, insert_idx));
                            }
                        }

                        if insert_idx == self.plugin_draft.steps.len() {
                            break;
                        }

                        let response = Self::render_plugin_step_card(
                            ui,
                            &format!("root_{insert_idx}"),
                            Some("root"),
                            insert_idx,
                            &mut self.plugin_draft.steps[insert_idx],
                            false,
                        );
                        if response.remove {
                            remove_idx = Some(insert_idx);
                        }
                        ui.add_space(4.0);
                    }
                }

                if let Some((from, mut to)) = move_request {
                    if from < self.plugin_draft.steps.len() {
                        if from < to {
                            to -= 1;
                        }
                        if from != to && to <= self.plugin_draft.steps.len() {
                            let step = self.plugin_draft.steps.remove(from);
                            self.plugin_draft.steps.insert(to, step);
                        }
                    }
                }
                if let Some(idx) = remove_idx {
                    if idx < self.plugin_draft.steps.len() {
                        self.plugin_draft.steps.remove(idx);
                    }
                }
            });

            columns[2].group(|ui| {
                ui.label(egui::RichText::new("Properties").strong());
                ui.label(
                    egui::RichText::new("Action metadata and derived variables.")
                        .weak()
                        .small(),
                );
                ui.add_space(6.0);
                Self::render_plugin_metadata_fields(ui, &mut self.plugin_draft);
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Variables").strong());
                let variable_names = Self::plugin_flow_variable_names(&self.plugin_draft.steps);
                if variable_names.is_empty() {
                    ui.label(egui::RichText::new("No output variables yet.").weak());
                } else {
                    for name in variable_names {
                        ui.label(format!("${name}"));
                    }
                }
            });
        });
    }

    fn render_plugin_json_editor(&mut self, ui: &mut egui::Ui) {
        match &self.plugin_editor_mode {
            PluginEditorMode::LowCode => {
                ui.horizontal(|ui| {
                    ui.label("Quicker Type:");
                    egui::ComboBox::from_id_salt("quicker_doc_kind")
                        .selected_text(match self.plugin_draft.kind {
                            LowCodePluginKind::KeyMacro => "ActionType 7: Key Macro",
                            LowCodePluginKind::OpenApp => "ActionType 11: Open App/File",
                            LowCodePluginKind::PluginFlow => "ActionType 24: Plugin Flow",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.plugin_draft.kind,
                                LowCodePluginKind::KeyMacro,
                                "ActionType 7: Key Macro",
                            );
                            ui.selectable_value(
                                &mut self.plugin_draft.kind,
                                LowCodePluginKind::OpenApp,
                                "ActionType 11: Open App/File",
                            );
                            ui.selectable_value(
                                &mut self.plugin_draft.kind,
                                LowCodePluginKind::PluginFlow,
                                "ActionType 24: Plugin Flow",
                            );
                        });
                });
                ui.add_space(6.0);

                if !matches!(self.plugin_draft.kind, LowCodePluginKind::PluginFlow) {
                    Self::render_plugin_metadata_fields(ui, &mut self.plugin_draft);
                    ui.add_space(8.0);
                }

                match self.plugin_draft.kind {
                    LowCodePluginKind::KeyMacro => {
                        ui.label(egui::RichText::new("Key Macro").strong());
                        ui.label(
                            egui::RichText::new(
                                "Build the macro with structured steps. The visual editor currently supports the runtime subset: Send Keys, Type Text, and Delay.",
                            )
                            .weak()
                            .small(),
                        );
                        ui.add_space(6.0);

                        let labels = Self::key_macro_step_kind_labels();
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("key_macro_new_step_kind")
                                .selected_text(labels[self.plugin_new_key_macro_step_idx])
                                .show_ui(ui, |ui| {
                                    for (idx, label) in labels.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.plugin_new_key_macro_step_idx,
                                            idx,
                                            *label,
                                        );
                                    }
                                });

                            if ui.button("Add Step").clicked() {
                                self.plugin_draft
                                    .key_macro_steps
                                    .push(Self::make_key_macro_step(
                                        self.plugin_new_key_macro_step_idx,
                                    ));
                            }
                        });

                        ui.add_space(8.0);
                        let mut remove_idx = None;
                        let mut move_request = None;
                        let drop_frame = egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(8, 4))
                            .stroke(egui::Stroke::new(
                                1.0,
                                ui.visuals().widgets.inactive.bg_stroke.color,
                            ));

                        if self.plugin_draft.key_macro_steps.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "No macro steps yet. Add one from the palette above.",
                                )
                                .weak(),
                            );
                        } else {
                            for insert_idx in 0..=self.plugin_draft.key_macro_steps.len() {
                                let (_, dropped) =
                                    ui.dnd_drop_zone::<StepDragPayload, _>(drop_frame, |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                egui::RichText::new("Drop step here")
                                                    .weak()
                                                    .small(),
                                            );
                                        });
                                    });
                                if let Some(payload) = dropped {
                                    if payload.scope == "key_macro_root" {
                                        move_request = Some((payload.from, insert_idx));
                                    }
                                }

                                if insert_idx == self.plugin_draft.key_macro_steps.len() {
                                    break;
                                }

                                let response = ui.dnd_drag_source(
                                    egui::Id::new(("key_macro_step", insert_idx)),
                                    StepDragPayload {
                                        scope: "key_macro_root".into(),
                                        from: insert_idx,
                                    },
                                    |ui| {
                                        Self::render_key_macro_step_card(
                                            ui,
                                            insert_idx,
                                            &mut self.plugin_draft.key_macro_steps[insert_idx],
                                        )
                                    },
                                );
                                if response.inner {
                                    remove_idx = Some(insert_idx);
                                }
                                ui.add_space(4.0);
                            }
                        }

                        if let Some((from, mut to)) = move_request {
                            if from < self.plugin_draft.key_macro_steps.len() {
                                if from < to {
                                    to -= 1;
                                }
                                if from != to && to <= self.plugin_draft.key_macro_steps.len() {
                                    let step = self.plugin_draft.key_macro_steps.remove(from);
                                    self.plugin_draft.key_macro_steps.insert(to, step);
                                }
                            }
                        }
                        if let Some(idx) = remove_idx {
                            if idx < self.plugin_draft.key_macro_steps.len() {
                                self.plugin_draft.key_macro_steps.remove(idx);
                            }
                        }
                    }
                    LowCodePluginKind::OpenApp => {
                        ui.label(egui::RichText::new("Open App / File").strong());
                        ui.label("Target path:");
                        ui.text_edit_singleline(&mut self.plugin_draft.launch_path);
                        ui.label("Arguments:");
                        ui.text_edit_singleline(&mut self.plugin_draft.launch_arguments);
                        ui.checkbox(
                            &mut self.plugin_draft.launch_set_working_dir,
                            "Use target folder as working directory",
                        );
                    }
                    LowCodePluginKind::PluginFlow => {
                        self.render_plugin_flow_builder(ui);
                    }
                }
            }
            PluginEditorMode::RawJson { reason } => {
                ui.label(egui::RichText::new("Raw JSON Mode").strong());
                ui.label(
                    egui::RichText::new(format!(
                        "This plugin cannot be imported into the low-code builder yet: {reason}"
                    ))
                    .weak()
                    .small(),
                );
                ui.add_space(6.0);
            }
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Raw JSON Import / Export").strong());
        ui.label(
            egui::RichText::new(match self.plugin_editor_mode {
                PluginEditorMode::LowCode =>
                    "Import a supported ActionType 7, 11, or 24 document into the builder, or export the current draft as native Quicker JSON.",
                PluginEditorMode::RawJson { .. } =>
                    "This plugin is currently using raw JSON mode. You can edit the JSON directly here, then save it, or try importing it into the builder again after simplifying unsupported steps.",
            })
            .weak()
            .small(),
        );
        ui.horizontal(|ui| {
            if matches!(self.plugin_editor_mode, PluginEditorMode::LowCode)
                && ui.button("Export Draft to JSON").clicked()
            {
                match self.plugin_draft.to_quicker_json() {
                    Ok(json) => {
                        self.edit_field1 = json;
                        self.show_toast("Draft exported to JSON".into(), false);
                    }
                    Err(err) => self.show_toast(err, true),
                }
            }
            if ui.button("Import JSON Into Builder").clicked() {
                match LowCodePluginDraft::from_quicker_plugin_json(&self.edit_field1) {
                    Ok(draft) => {
                        self.plugin_draft = draft;
                        self.plugin_editor_mode = PluginEditorMode::LowCode;
                        self.show_toast("Imported JSON into the builder".into(), false);
                    }
                    Err(err) => {
                        self.plugin_editor_mode = PluginEditorMode::RawJson {
                            reason: err.clone(),
                        };
                        self.show_toast(err, true);
                    }
                }
            }
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.edit_field1)
                .desired_width(f32::INFINITY)
                .desired_rows(12)
                .code_editor(),
        );
    }

    pub(super) fn render_action_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("← Cancel").clicked() {
                self.edit_target = None;
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            }
            ui.heading(if self.edit_target.is_some() {
                "Edit Plugin"
            } else {
                "Add Plugin"
            });
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("action_editor_scroll")
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {}",
                        if self.edit_target.is_some() {
                            "Editing in"
                        } else {
                            "Adding into"
                        },
                        self.add_action_target_label()
                    ))
                    .weak()
                    .small(),
                );
                ui.add_space(8.0);

                self.render_plugin_json_editor(ui);

                ui.add_space(16.0);

                if ui.button("✓ Save Plugin").clicked() {
                    let action_result = match self.plugin_editor_mode {
                        PluginEditorMode::LowCode => self.plugin_draft.to_action(),
                        PluginEditorMode::RawJson { .. } => {
                            Action::from_quicker_plugin_json(&self.edit_field1)
                        }
                    };
                    let action = match action_result {
                        Ok(action) => action,
                        Err(err) => {
                            self.show_toast(err, true);
                            return;
                        }
                    };

                    self.persist_edited_or_new_action(action);
                }
            });
    }

    pub(super) fn render_script_output(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            }
            ui.heading("Script Output");
            if ui.button("📋 Copy").clicked() {
                ui.ctx().copy_text(self.script_output.clone());
                self.show_toast("Copied!".into(), false);
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("script_output_scroll")
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.script_output.as_str())
                        .desired_width(f32::INFINITY)
                        .code_editor(),
                );
            });
    }
}
