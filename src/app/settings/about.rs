use super::*;

impl QuickerApp {
    pub(super) fn render_about_settings_page(&mut self, ui: &mut egui::Ui) {
        settings_card(
            ui,
            "How This Page Works",
            "The layout follows the Quicker-style control page while keeping the app's real config model underneath.",
            |ui| {
                ui.label("Launcher: shortcut, grid columns, and panel size.");
                ui.label("Profiles: app-aware routing for the lower tool section.");
                ui.label("Data & Storage: save, inspect, and open config files.");
            },
        );

        settings_card(
            ui,
            "Keyboard Notes",
            "A few behaviors matter when switching between the panel and settings view.",
            |ui| {
                ui.label("Press Escape to leave settings and return to the launcher.");
                ui.label("Apply Settings saves the current config file to disk.");
                ui.label("Global hotkey edits are persisted here; some desktops may need a restart to rebind them.");
            },
        );
    }
}
