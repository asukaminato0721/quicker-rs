use super::*;

impl QuickerApp {
    pub(super) fn render_profile_settings_page(
        &mut self,
        ui: &mut egui::Ui,
        current_process_alias: Option<&str>,
        current_process_label: &str,
    ) {
        settings_card(
            ui,
            "Profile Matching",
            "The first profile is fixed as the Global Tools section. Every additional profile can target one or more focused process names.",
            |ui| {
                ui.label(format!("Current focused process: {}", current_process_label));
                if let Some(alias) = current_process_alias {
                    ui.label(format!("Suggested match alias: {}", alias));
                }
            },
        );

        let mut to_delete = None;
        let mut profile_rules_changed = false;
        let can_delete_profiles = self.config.profiles.len() > 1;

        for (i, profile) in self.config.profiles.iter_mut().enumerate() {
            settings_card(
                ui,
                &format!("Profile {}", i + 1),
                if i == 0 {
                    "Pinned global tools profile."
                } else {
                    "Used when one of its match rules matches the focused process."
                },
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut profile.name)
                                .desired_width(220.0)
                                .hint_text("Profile name"),
                        );
                        settings_badge(ui, format!("{} actions", profile.actions.len()));
                        if can_delete_profiles && i > 0 && ui.small_button("Delete").clicked() {
                            to_delete = Some(i);
                        }
                    });

                    if i == 0 {
                        ui.label(
                            egui::RichText::new(
                                "This profile always renders in the upper Global Tools section and does not use match rules.",
                            )
                            .small()
                            .weak(),
                        );
                    } else {
                        ui.add_space(6.0);
                        ui.label("Match focused processes (comma-separated)");
                        let mut process_matches = profile.match_processes.join(", ");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut process_matches)
                                    .desired_width(ui.available_width())
                                    .hint_text("firefox, chrome, code"),
                            )
                            .changed()
                        {
                            profile.match_processes = process_matches
                                .split(',')
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_string)
                                .collect();
                            profile_rules_changed = true;
                        }
                        if let Some(alias) = current_process_alias {
                            ui.add_space(6.0);
                            if ui.small_button(format!("Use {}", alias)).clicked()
                                && !profile
                                    .match_processes
                                    .iter()
                                    .any(|value| value.eq_ignore_ascii_case(alias))
                            {
                                profile.match_processes.push(alias.into());
                                profile_rules_changed = true;
                            }
                        }
                    }
                },
            );
        }

        if let Some(idx) = to_delete {
            self.config.profiles.remove(idx);
            if let Some(scope) = &mut self.action_scope {
                if scope.profile_idx == idx {
                    self.action_scope = None;
                } else if scope.profile_idx > idx {
                    scope.profile_idx -= 1;
                }
            }
            if self.active_profile >= self.config.profiles.len() {
                self.active_profile = 0;
            } else if self.active_profile > idx {
                self.active_profile -= 1;
            }
            self.needs_focus_profile_sync = true;
        }

        if ui.button("Add Profile").clicked() {
            self.config.profiles.push(crate::config::Profile {
                name: format!("Profile {}", self.config.profiles.len() + 1),
                description: String::new(),
                match_processes: vec![],
                actions: vec![],
            });
            self.needs_focus_profile_sync = true;
        }

        if profile_rules_changed {
            self.needs_focus_profile_sync = true;
        }
    }
}
