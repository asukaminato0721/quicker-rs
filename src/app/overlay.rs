use super::*;

impl QuickerApp {
    pub(super) fn render_toast(&mut self, ctx: &egui::Context) {
        if let Some(toast) = &self.toast {
            if std::time::Instant::now() > toast.expires {
                self.toast = None;
                return;
            }
            egui::Area::new(egui::Id::new("toast"))
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -20.0))
                .show(ctx, |ui| {
                    let color = if toast.is_error {
                        egui::Color32::from_rgb(220, 50, 50)
                    } else {
                        egui::Color32::from_rgb(50, 160, 80)
                    };
                    egui::Frame::new()
                        .fill(color)
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(16, 8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&toast.message)
                                    .color(egui::Color32::WHITE)
                                    .size(14.0),
                            );
                        });
                });
            ctx.request_repaint();
        }
    }

    pub(super) fn render_running_action(&mut self, ctx: &egui::Context) {
        let Some(action_name) = self.pending_action_name.clone() else {
            return;
        };
        let is_cancelling = self
            .action_control
            .as_ref()
            .is_some_and(ActionExecutionControl::is_cancelled);

        egui::Area::new(egui::Id::new("running_action"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(32, 64, 110))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new(if is_cancelling {
                                    format!("Cancelling {action_name}")
                                } else {
                                    format!("Running {action_name}")
                                })
                                .color(egui::Color32::WHITE)
                                .size(14.0),
                            );
                            let button =
                                ui.add_enabled(!is_cancelling, egui::Button::new("Cancel"));
                            if button.clicked() {
                                self.request_cancel_running_action(ctx);
                            }
                        });
                    });
            });
        ctx.request_repaint();
    }

    pub(super) fn handle_global_hotkey(&mut self, ctx: &egui::Context) {
        let Some(toggle_hotkey) = self.toggle_hotkey else {
            return;
        };

        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id() == toggle_hotkey.id() && event.state() == HotKeyState::Pressed {
                self.panel_hidden = !self.panel_hidden;
                self.restore_panel_window(ctx);
                if !self.panel_hidden {
                    self.view = View::Panel;
                    self.needs_focus_profile_sync = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
    }

    pub(super) fn show_startup_notice_once(&mut self) {
        if let Some((message, is_error)) = self.startup_notice.take() {
            self.show_toast(message, is_error);
        }
    }

    pub(super) fn poll_action_result(&mut self) {
        let Some(rx) = &self.action_result_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(message) => {
                self.action_control = None;
                self.pending_action_name = None;
                self.action_result_rx = None;
                self.handle_exec_result(&message.action_name, message.result);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let action_name = self
                    .pending_action_name
                    .take()
                    .unwrap_or_else(|| "Action".into());
                self.action_control = None;
                self.action_result_rx = None;
                self.show_toast(format!("{action_name} stopped unexpectedly"), true);
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(super) fn render_radial_menu(&self, ctx: &egui::Context) {
        let Some(menu) = &self.radial_menu else {
            return;
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("radial_menu"),
        ));
        let screen = ctx.content_rect();
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(24));

        let (inner_count, outer_count) = radial_ring_counts(menu.entries.len());
        let hovered = radial_hovered_entry(menu.origin, menu.pointer, (inner_count, outer_count));

        if inner_count > 0 {
            paint_radial_ring(
                &painter,
                menu.origin,
                &menu.entries[..inner_count],
                0,
                RADIAL_CENTER_RADIUS,
                RADIAL_INNER_RADIUS,
                hovered,
            );
        }

        if outer_count > 0 {
            paint_radial_ring(
                &painter,
                menu.origin,
                &menu.entries[inner_count..],
                inner_count,
                RADIAL_INNER_RADIUS,
                RADIAL_OUTER_RADIUS,
                hovered,
            );
        }

        painter.circle_filled(menu.origin, RADIAL_CENTER_RADIUS, egui::Color32::WHITE);
        painter.circle_stroke(
            menu.origin,
            RADIAL_CENTER_RADIUS,
            egui::Stroke::new(1.0, egui::Color32::from_gray(180)),
        );
        painter.text(
            menu.origin,
            egui::Align2::CENTER_CENTER,
            "Cancel",
            egui::FontId::proportional(16.0),
            egui::Color32::from_rgb(200, 70, 60),
        );
    }
}
