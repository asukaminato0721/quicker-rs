use super::*;

mod about;
mod launcher;
mod profiles;
mod storage;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsPage {
    Launcher,
    Profiles,
    Storage,
    About,
}

impl SettingsPage {
    pub(super) fn all() -> [Self; 4] {
        [Self::Launcher, Self::Profiles, Self::Storage, Self::About]
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Launcher => "Launcher",
            Self::Profiles => "Profiles",
            Self::Storage => "Data & Storage",
            Self::About => "About",
        }
    }

    pub(super) fn summary(self) -> &'static str {
        match self {
            Self::Launcher => "Shortcut, panel size, and focus behavior.",
            Self::Profiles => "Route tools to the right app context.",
            Self::Storage => "Config path, save actions, and local stats.",
            Self::About => "Keyboard hints and page guidance.",
        }
    }

    pub(super) fn rail_icon(self) -> &'static str {
        match self {
            Self::Launcher => "⌘",
            Self::Profiles => "◫",
            Self::Storage => "▣",
            Self::About => "?",
        }
    }

    pub(super) fn matches_query(self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }

        self.title().to_ascii_lowercase().contains(&query)
            || self.summary().to_ascii_lowercase().contains(&query)
    }
}

impl QuickerApp {
    pub(super) fn render_settings(&mut self, ui: &mut egui::Ui) {
        let current_process = self.focus_tracker.current_external().cloned();
        let current_process_alias = current_process
            .as_ref()
            .map(|process| process.primary_alias());
        let current_process_label = current_process
            .as_ref()
            .map(|process| process.display_name())
            .unwrap_or_else(|| "Unavailable".into());

        let visible_pages: Vec<_> = SettingsPage::all()
            .into_iter()
            .filter(|page| page.matches_query(&self.settings_search))
            .collect();
        if !visible_pages.contains(&self.settings_page) {
            self.settings_page = visible_pages
                .first()
                .copied()
                .unwrap_or(SettingsPage::Launcher);
        }

        let shell_fill = egui::Color32::from_rgb(244, 245, 247);
        let side_fill = egui::Color32::from_rgb(236, 238, 241);
        let nav_fill = egui::Color32::from_rgb(242, 243, 246);
        let main_fill = egui::Color32::from_rgb(252, 252, 253);

        egui::Frame::new()
            .fill(shell_fill)
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                let target_height = ui.available_height().max(560.0);
                let total_width = ui.available_width();
                let icon_width = 56.0;
                let nav_width = (total_width * 0.22).clamp(180.0, 240.0);
                let main_width = (total_width - icon_width - nav_width - 20.0).max(420.0);
                let pane_height = target_height - 20.0;

                ui.set_min_height(target_height);
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(icon_width, pane_height),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            egui::Frame::new()
                                .fill(side_fill)
                                .corner_radius(egui::CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(icon_width - 16.0, pane_height - 16.0));
                                    ui.vertical_centered(|ui| {
                                        let (avatar_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(32.0, 32.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            avatar_rect.center(),
                                            16.0,
                                            egui::Color32::from_rgb(91, 134, 204),
                                        );
                                        ui.painter().text(
                                            avatar_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "Q",
                                            egui::FontId::proportional(16.0),
                                            egui::Color32::WHITE,
                                        );
                                        ui.add_space(18.0);

                                        for page in SettingsPage::all() {
                                            let selected = self.settings_page == page;
                                            let button = egui::Button::new(
                                                egui::RichText::new(page.rail_icon())
                                                    .size(18.0)
                                                    .color(if selected {
                                                        egui::Color32::from_rgb(42, 95, 183)
                                                    } else {
                                                        egui::Color32::from_rgb(119, 126, 141)
                                                    }),
                                            )
                                            .min_size(egui::vec2(36.0, 36.0))
                                            .fill(if selected {
                                                egui::Color32::from_rgb(223, 232, 248)
                                            } else {
                                                egui::Color32::TRANSPARENT
                                            })
                                            .stroke(egui::Stroke::new(
                                                1.0,
                                                if selected {
                                                    egui::Color32::from_rgb(180, 200, 233)
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                },
                                            ))
                                            .corner_radius(egui::CornerRadius::same(10));

                                            if ui.add(button).clicked() {
                                                self.settings_page = page;
                                            }
                                            ui.add_space(6.0);
                                        }
                                    });
                                });
                        },
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2(nav_width, pane_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::new()
                                .fill(nav_fill)
                                .corner_radius(egui::CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(nav_width - 24.0, pane_height - 24.0));

                                    ui.label(
                                        egui::RichText::new("Quicker Control")
                                            .size(16.0)
                                            .strong()
                                            .color(egui::Color32::from_rgb(46, 54, 69)),
                                    );
                                    ui.label(
                                        egui::RichText::new("Filter and jump to a settings section.")
                                            .small()
                                            .color(egui::Color32::from_rgb(126, 132, 145)),
                                    );
                                    ui.add_space(10.0);
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.settings_search)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("Search settings"),
                                    );
                                    ui.add_space(14.0);
                                    ui.label(
                                        egui::RichText::new("Sections")
                                            .small()
                                            .strong()
                                            .color(egui::Color32::from_rgb(94, 100, 112)),
                                    );
                                    ui.add_space(6.0);

                                    if visible_pages.is_empty() {
                                        ui.label(egui::RichText::new("No matching section").small().weak());
                                    } else {
                                        for page in visible_pages {
                                            let selected = self.settings_page == page;
                                            let fill = if selected {
                                                egui::Color32::from_rgb(225, 234, 248)
                                            } else {
                                                egui::Color32::from_rgba_premultiplied(255, 255, 255, 0)
                                            };
                                            let stroke = egui::Stroke::new(
                                                1.0,
                                                if selected {
                                                    egui::Color32::from_rgb(180, 199, 230)
                                                } else {
                                                    egui::Color32::from_rgb(225, 227, 232)
                                                },
                                            );
                                            if ui
                                                .add_sized(
                                                    [ui.available_width(), 34.0],
                                                    egui::Button::new(
                                                        egui::RichText::new(page.title())
                                                            .strong()
                                                            .color(if selected {
                                                                egui::Color32::from_rgb(41, 83, 156)
                                                            } else {
                                                                egui::Color32::from_rgb(58, 65, 78)
                                                            }),
                                                    )
                                                    .fill(fill)
                                                    .stroke(stroke)
                                                    .corner_radius(egui::CornerRadius::same(8)),
                                                )
                                                .clicked()
                                            {
                                                self.settings_page = page;
                                            }
                                            ui.add_space(2.0);
                                            ui.label(
                                                egui::RichText::new(page.summary())
                                                    .small()
                                                    .color(egui::Color32::from_rgb(126, 132, 145)),
                                            );
                                            ui.add_space(10.0);
                                        }
                                    }
                                });
                        },
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2(main_width, pane_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::new()
                                .fill(main_fill)
                                .corner_radius(egui::CornerRadius::same(12))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(226, 228, 233),
                                ))
                                .inner_margin(egui::Margin::same(16))
                                .show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(main_width - 32.0, pane_height - 32.0));

                                    ui.horizontal(|ui| {
                                        if ui.button("←").clicked() {
                                            self.view = View::Panel;
                                            self.needs_focus_profile_sync = true;
                                        }
                                        ui.add_space(4.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                egui::RichText::new(self.settings_page.title())
                                                    .size(20.0)
                                                    .strong()
                                                    .color(egui::Color32::from_rgb(35, 43, 57)),
                                            );
                                            ui.label(
                                                egui::RichText::new(self.settings_page.summary())
                                                    .small()
                                                    .color(egui::Color32::from_rgb(124, 130, 144)),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("?").strong().color(
                                                                egui::Color32::from_rgb(63, 109, 188),
                                                            ),
                                                        )
                                                        .min_size(egui::vec2(28.0, 28.0))
                                                        .fill(egui::Color32::from_rgb(233, 239, 250))
                                                        .stroke(egui::Stroke::new(
                                                            1.0,
                                                            egui::Color32::from_rgb(194, 209, 236),
                                                        ))
                                                        .corner_radius(egui::CornerRadius::same(14)),
                                                    )
                                                    .clicked()
                                                {
                                                    self.show_toast(
                                                        "Edit values, then apply settings below."
                                                            .into(),
                                                        false,
                                                    );
                                                }
                                            },
                                        );
                                    });

                                    let (accent_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(76.0, 3.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        accent_rect,
                                        2.0,
                                        egui::Color32::from_rgb(57, 109, 204),
                                    );
                                    ui.add_space(8.0);

                                    egui::ScrollArea::vertical()
                                        .id_salt("settings_scroll")
                                        .max_height((target_height - 128.0).max(240.0))
                                        .show(ui, |ui| match self.settings_page {
                                            SettingsPage::Launcher => {
                                                self.render_launcher_settings_page(
                                                    ui,
                                                    &current_process_label,
                                                )
                                            }
                                            SettingsPage::Profiles => self.render_profile_settings_page(
                                                ui,
                                                current_process_alias.as_deref(),
                                                &current_process_label,
                                            ),
                                            SettingsPage::Storage => {
                                                self.render_storage_settings_page(
                                                    ui,
                                                    &current_process_label,
                                                )
                                            }
                                            SettingsPage::About => self.render_about_settings_page(ui),
                                        });

                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Apply Settings")
                                                            .strong()
                                                            .color(egui::Color32::WHITE),
                                                    )
                                                    .min_size(egui::vec2(152.0, 36.0))
                                                    .fill(egui::Color32::from_rgb(56, 110, 205))
                                                    .stroke(egui::Stroke::NONE)
                                                    .corner_radius(egui::CornerRadius::same(10)),
                                                )
                                                .clicked()
                                            {
                                                self.config.save();
                                                self.show_toast("Config saved.".into(), false);
                                                self.needs_focus_profile_sync = true;
                                            }

                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "Config: {}",
                                                    Config::config_path().display()
                                                ))
                                                .small()
                                                .color(egui::Color32::from_rgb(126, 132, 145)),
                                            );
                                        },
                                    );
                                });
                        },
                    );
                });
            });
    }
}

pub(super) fn settings_card<R>(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(247, 248, 250))
        .corner_radius(egui::CornerRadius::same(12))
        .stroke(egui::Stroke::new(
            1.0,
            egui::Color32::from_rgb(227, 229, 234),
        ))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(15.0)
                    .strong()
                    .color(egui::Color32::from_rgb(45, 52, 66)),
            );
            ui.label(
                egui::RichText::new(subtitle)
                    .small()
                    .color(egui::Color32::from_rgb(126, 132, 145)),
            );
            ui.add_space(10.0);
            add_contents(ui)
        })
        .inner
}

pub(super) fn settings_badge(ui: &mut egui::Ui, text: impl Into<String>) {
    let text = text.into();
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(233, 238, 246))
        .corner_radius(egui::CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .small()
                    .color(egui::Color32::from_rgb(63, 92, 138)),
            );
        });
}
