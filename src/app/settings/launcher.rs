use super::*;

impl QuickerApp {
    pub(super) fn render_launcher_settings_page(
        &mut self,
        ui: &mut egui::Ui,
        current_process_label: &str,
    ) {
        settings_card(
            ui,
            "Launcher Shortcut",
            "The global shortcut opens the main panel. Changes are saved immediately but may require a restart to rebind on every desktop session.",
            |ui| {
                ui.label("Toggle Hotkey");
                ui.add(
                    egui::TextEdit::singleline(&mut self.config.toggle_hotkey)
                        .desired_width(220.0)
                        .hint_text("Alt+Space"),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    settings_badge(ui, format!("Current value: {}", self.config.toggle_hotkey));
                    settings_badge(ui, "Global launcher trigger");
                });
            },
        );

        settings_card(
            ui,
            "Panel Layout",
            "Tune the launcher density and the canvas size for the command grid.",
            |ui| {
                ui.label(format!("Grid Columns: {}", self.config.columns));
                ui.add(egui::Slider::new(&mut self.config.columns, 2..=8));
                ui.add_space(6.0);

                ui.label(format!("Panel Width: {:.0}px", self.config.panel_width));
                ui.add(egui::Slider::new(
                    &mut self.config.panel_width,
                    300.0..=1200.0,
                ));
                ui.add_space(6.0);

                ui.label(format!("Panel Height: {:.0}px", self.config.panel_height));
                ui.add(egui::Slider::new(
                    &mut self.config.panel_height,
                    200.0..=900.0,
                ));
                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    settings_badge(
                        ui,
                        format!(
                            "{} x {:.0}px x {:.0}px",
                            self.config.columns, self.config.panel_width, self.config.panel_height
                        ),
                    );
                    settings_badge(ui, "Large panels work best with dense action sets");
                });
            },
        );

        let matched_profile = self
            .active_window_profile_index()
            .and_then(|idx| self.config.profiles.get(idx))
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "No app-specific profile".into());

        settings_card(
            ui,
            "Focus Routing",
            "Profiles below the fixed global section are activated from the currently focused external application.",
            |ui| {
                ui.label(format!("Focused process: {}", current_process_label));
                ui.label(format!("Matched profile: {}", matched_profile));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    settings_badge(ui, "Top section is always Global Tools");
                    settings_badge(ui, "Bottom section follows focused app rules");
                });
            },
        );
    }
}
