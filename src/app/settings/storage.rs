use super::*;

impl QuickerApp {
    pub(super) fn render_storage_settings_page(
        &mut self,
        ui: &mut egui::Ui,
        current_process_label: &str,
    ) {
        let config_path = Config::config_path();
        let config_dir = config_path
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| ".".into());
        let total_actions: usize = self
            .config
            .profiles
            .iter()
            .map(|profile| profile.actions.len())
            .sum();

        settings_card(
            ui,
            "Configuration File",
            "Use these shortcuts to inspect the persisted config and its storage folder.",
            |ui| {
                ui.label(format!("Config file: {}", config_path.display()));
                ui.label(format!("Config folder: {}", config_dir));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Save Config Now").clicked() {
                        self.config.save();
                        self.show_toast("Config saved.".into(), false);
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Open Config File").clicked() {
                        if let Err(err) = open::that(&config_path) {
                            self.show_toast(format!("Failed to open config file: {}", err), true);
                        }
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Open Config Folder").clicked() {
                        if let Some(parent) = config_path.parent() {
                            if let Err(err) = open::that(parent) {
                                self.show_toast(
                                    format!("Failed to open config folder: {}", err),
                                    true,
                                );
                            }
                        }
                    }

                    #[cfg(target_arch = "wasm32")]
                    ui.label(
                        egui::RichText::new(
                            "Browser preview uses an in-memory config. Native file access is disabled.",
                        )
                        .small()
                        .weak(),
                    );
                });
            },
        );

        settings_card(
            ui,
            "Local Snapshot",
            "A quick overview of what this config currently drives in the launcher.",
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    settings_badge(ui, format!("{} profiles", self.config.profiles.len()));
                    settings_badge(ui, format!("{} total actions", total_actions));
                    settings_badge(ui, format!("Focused process: {}", current_process_label));
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Profile edits take effect in the panel view as soon as the focus sync runs again.",
                    )
                    .small()
                    .weak(),
                );
            },
        );
    }
}
