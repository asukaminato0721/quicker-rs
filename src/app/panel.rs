use super::*;

impl QuickerApp {
    fn render_action_results_grid(
        &mut self,
        ui: &mut egui::Ui,
        grid_id: &str,
        entries: &[ActionListEntry],
    ) {
        let cols = self.config.columns;
        let mut clicked_action: Option<ActionListEntry> = None;
        let mut edit_action: Option<ActionListEntry> = None;
        let mut delete_action: Option<ActionEditTarget> = None;

        egui::Grid::new(grid_id)
            .num_columns(cols)
            .spacing([8.0, 8.0])
            .min_col_width(ui.available_width() / cols as f32 - 8.0)
            .show(ui, |ui| {
                for (i, entry) in entries.iter().enumerate() {
                    if i > 0 && i % cols == 0 {
                        ui.end_row();
                    }
                    let btn_width = ui
                        .available_width()
                        .min((self.config.panel_width - 32.0) / cols as f32 - 8.0);

                    let icon = entry
                        .action
                        .icon
                        .as_deref()
                        .unwrap_or(default_action_icon(&entry.action));
                    let label = match &entry.action.kind {
                        ActionKind::Group { .. } => format!("{} {} ›", icon, entry.action.name),
                        _ => format!("{} {}", icon, entry.action.name),
                    };

                    ui.vertical(|ui| {
                        let btn = egui::Button::new(egui::RichText::new(&label).size(14.0))
                            .min_size(egui::vec2(btn_width, 48.0));

                        let response = ui.add(btn);

                        if !entry.action.description.is_empty() {
                            response.clone().on_hover_text(&entry.action.description);
                        }

                        if response.clicked() {
                            clicked_action = Some(entry.clone());
                        }

                        if matches!(entry.action.kind, ActionKind::PluginPipeline { .. }) {
                            ui.horizontal(|ui| {
                                if ui.small_button("Edit").clicked() {
                                    edit_action = Some(entry.clone());
                                }
                                if ui.small_button("Delete").clicked() {
                                    delete_action = Some(ActionEditTarget {
                                        profile_idx: entry.profile_idx,
                                        path: entry.path.clone(),
                                        action_idx: entry.action_idx,
                                    });
                                }
                            });
                        }
                    });
                }
            });

        if let Some(target) = delete_action {
            if self.delete_action(&target) {
                self.config.save();
                self.show_toast("Plugin deleted!".into(), false);
                self.needs_focus_profile_sync = true;
            } else {
                self.show_toast("Failed to delete plugin.".into(), true);
            }
            return;
        }

        if let Some(entry) = edit_action {
            self.open_plugin_editor_for_entry(&entry);
            return;
        }

        if let Some(entry) = clicked_action {
            self.trigger_action(
                ui.ctx(),
                entry.profile_idx,
                entry.section,
                entry.action_idx,
                entry.action,
            );
        }
    }

    fn render_action_section(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        empty_message: &str,
        section_id: &str,
        entries: &[ActionListEntry],
        height: f32,
    ) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_height(height);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(title).strong());
                if !subtitle.is_empty() {
                    ui.label(egui::RichText::new(subtitle).weak().small());
                }
            });
            ui.add_space(6.0);

            if entries.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(egui::RichText::new(empty_message).weak());
                });
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt(format!("{section_id}_scroll"))
                .max_height((height - 32.0).max(80.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.render_action_results_grid(ui, section_id, entries)
                });
        });
    }

    pub(super) fn render_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let focused_app = self
                .focus_tracker
                .current_external()
                .map(|process| process.display_name())
                .unwrap_or_else(|| "Unavailable".into());
            let active_profile_name = self
                .active_window_profile_index()
                .and_then(|idx| self.config.profiles.get(idx))
                .map(|profile| profile.name.clone())
                .unwrap_or_else(|| "None".into());

            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Upper: Global tools").strong());
                ui.label(
                    egui::RichText::new(format!(
                        "Lower: tools for focused app ({focused_app}) -> {active_profile_name}"
                    ))
                    .weak()
                    .small(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙").on_hover_text("Settings").clicked() {
                    self.view = View::Settings;
                }
                if ui.button("＋").on_hover_text("Add plugin").clicked() {
                    self.reset_editor();
                    self.view = View::ActionEditor;
                }
            });
        });

        ui.separator();

        if let Some(scope) = self.action_scope.clone() {
            ui.horizontal_wrapped(|ui| {
                if ui.button("← Back").clicked() {
                    self.leave_group();
                }
                ui.label(egui::RichText::new(scope.section.label()).weak());
                if let Some(profile) = self.config.profiles.get(scope.profile_idx) {
                    ui.label(egui::RichText::new("›").weak());
                    ui.label(egui::RichText::new(&profile.name).weak());
                }
                for title in self.group_titles() {
                    ui.label(egui::RichText::new("›").weak());
                    ui.label(title);
                }
            });
            ui.add_space(4.0);
        }

        let search_response = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("🔍 Search actions...")
                .desired_width(f32::INFINITY),
        );
        if ui.memory(|m| m.focused().is_none()) {
            search_response.request_focus();
        }

        ui.add_space(6.0);

        if self.action_scope.is_some() {
            let entries = self.filtered_entries(&self.current_action_entries());
            if entries.len() == 1 && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let entry = entries[0].clone();
                self.trigger_action(
                    ui.ctx(),
                    entry.profile_idx,
                    entry.section,
                    entry.action_idx,
                    entry.action,
                );
                return;
            }

            if entries.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(egui::RichText::new("No actions found").size(16.0).weak());
                    ui.add_space(8.0);
                    ui.label("Add some actions or adjust your search.");
                });
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("action_scope_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.render_action_results_grid(ui, "action_grid", &entries)
                });
            return;
        }

        let global_profile_idx = self.global_profile_idx();
        let global_profile_name = self
            .config
            .profiles
            .get(global_profile_idx)
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "Default".into());
        let global_entries = self.filtered_entries(&self.action_entries(
            global_profile_idx,
            ActionSection::GlobalTools,
            Vec::new(),
            self.profile_actions(global_profile_idx),
        ));

        let active_window_entries = self
            .active_window_profile_index()
            .map(|profile_idx| {
                self.filtered_entries(&self.action_entries(
                    profile_idx,
                    ActionSection::ActiveWindowTools,
                    Vec::new(),
                    self.profile_actions(profile_idx),
                ))
            })
            .unwrap_or_default();

        let total_results = global_entries.len() + active_window_entries.len();
        if total_results == 1 && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let entry = global_entries
                .first()
                .cloned()
                .or_else(|| active_window_entries.first().cloned())
                .unwrap();
            self.trigger_action(
                ui.ctx(),
                entry.profile_idx,
                entry.section,
                entry.action_idx,
                entry.action,
            );
            return;
        }

        if total_results == 0 {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(egui::RichText::new("No actions found").size(16.0).weak());
                ui.add_space(8.0);
                ui.label("Add some actions or adjust your search.");
            });
            return;
        }

        let available_height = ui.available_height();
        let section_height = ((available_height - 12.0) * 0.5).max(140.0);

        self.render_action_section(
            ui,
            ActionSection::GlobalTools.label(),
            &global_profile_name,
            "No global tools configured.",
            "global_tools_grid",
            &global_entries,
            section_height,
        );

        ui.add_space(8.0);

        let current_window_subtitle = self
            .active_window_profile_index()
            .and_then(|idx| self.config.profiles.get(idx))
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "No matching app-specific profile".into());
        let current_window_empty = self
            .focus_tracker
            .current_external()
            .map(|process| format!("No app-specific tools for {}", process.display_name()))
            .unwrap_or_else(|| "Focus another app to load its tools here.".into());

        self.render_action_section(
            ui,
            ActionSection::ActiveWindowTools.label(),
            &current_window_subtitle,
            &current_window_empty,
            "current_window_tools_grid",
            &active_window_entries,
            section_height,
        );
    }
}
