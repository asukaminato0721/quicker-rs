use super::*;

pub(crate) fn render(ctx: &Context) {
    let host = HOST.get_or_init(|| Host {
        context: ctx.clone(),
        windows: Mutex::new(Vec::new()),
    });
    let windows: Vec<_> = host
        .windows
        .lock()
        .unwrap()
        .iter()
        .filter_map(Weak::upgrade)
        .collect();
    for handle in windows {
        let (id, builder) = {
            let mut window = handle.lock().unwrap();
            if window.result.closed {
                continue;
            }
            #[cfg(target_os = "linux")]
            if window.position.is_none() {
                match crate::x11::wait_window::geometry(window.scale) {
                    Ok((area, mouse)) => {
                        window.position = Some(placement(
                            &window.options.location,
                            area,
                            mouse,
                            egui::vec2(420.0, 180.0),
                        ))
                    }
                    Err(error) => {
                        window.result.error = Some(error);
                        window.result.closed = true;
                        continue;
                    }
                }
            }
            // Configure the X11 input hints before mapping the window. The temporary
            // title identifies only this viewport, even when action titles are equal.
            let title = if window.configured {
                window.content.title.clone()
            } else {
                window.startup_title()
            };
            (
                viewport(window.id),
                egui::ViewportBuilder::default()
                    .with_title(title)
                    .with_position(window.position.unwrap_or_default())
                    .with_visible(window.configured)
                    .with_inner_size([420.0, 180.0])
                    .with_min_inner_size([260.0, 120.0])
                    .with_active(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop),
            )
        };
        ctx.show_viewport_deferred(id, builder, move |ctx, _| handle.lock().unwrap().ui(ctx));
    }
}

impl Window {
    fn startup_title(&self) -> String {
        format!("quicker-wait-init-{}-{}", std::process::id(), self.id)
    }

    fn configure(&mut self, ctx: &Context) -> Result<bool, String> {
        #[cfg(target_os = "linux")]
        {
            let Some(window) = crate::x11::wait_window::configure(
                &self.startup_title(),
                &self.options.activation,
            )?
            else {
                return Ok(false);
            };
            self.native_window = Some(window);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.content.title.clone()));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            if self.options.activation == "AutoActivate" {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Ok(true)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = ctx;
            Err("Wait windows currently require Linux X11".into())
        }
    }

    fn ui(&mut self, ctx: &Context) {
        if self.control.is_cancelled() {
            self.result.closed = true;
        }
        if !self.result.closed && !self.configured {
            match self.configure(ctx) {
                Ok(true) => {
                    self.configured = true;
                    repaint(self.id);
                }
                Ok(false) if self.created.elapsed() < Duration::from_secs(5) => {
                    ctx.request_repaint_after(Duration::from_millis(20));
                    return;
                }
                Ok(false) => {
                    self.result.error = Some("Cannot locate the native wait window".into());
                    self.result.closed = true;
                }
                Err(error) => {
                    self.result.error = Some(error);
                    self.result.closed = true;
                }
            }
        }
        if self.result.closed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.result.shown {
            self.result.shown = true;
            self.shown_at = Some(Instant::now());
        }
        let expired = self.options.auto_close > 0.0
            && self
                .shown_at
                .is_some_and(|start| start.elapsed().as_secs_f64() >= self.options.auto_close);
        if ctx.input(|i| i.viewport().close_requested()) || expired {
            self.dismiss(None);
        }
        if self.result.closed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let mut selected = None;
        egui::TopBottomPanel::bottom(egui::Id::new(("wait-buttons", self.id))).show(ctx, |ui| {
            ui.style_mut().override_font_id =
                Some(egui::FontId::proportional(self.options.font_size));
            egui::ScrollArea::vertical()
                .id_salt(("wait-button-scroll", self.id))
                .max_height(100.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (label, value) in &self.content.operations {
                            if ui.button(label).clicked() {
                                selected = Some(value.clone());
                            }
                        }
                        if !self.content.button.is_empty()
                            && ui.button(&self.content.button).clicked()
                        {
                            selected = Some(String::new());
                        }
                    });
                });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().override_font_id =
                Some(egui::FontId::proportional(self.options.font_size));
            egui::ScrollArea::vertical()
                .id_salt(("wait-content", self.id))
                .show(ui, |ui| {
                    ui.label(&self.content.prompt);
                    if let Some(progress) = self.content.progress {
                        ui.add(egui::ProgressBar::new(progress).text(&self.content.progress_text));
                    }
                    if self.options.auto_close > 0.0 {
                        let elapsed = self.shown_at.unwrap().elapsed().as_secs_f64();
                        ui.add(
                            egui::ProgressBar::new(
                                (elapsed / self.options.auto_close).min(1.0) as f32
                            )
                            .text(format!(
                                "{:.1} s",
                                (self.options.auto_close - elapsed).max(0.0)
                            )),
                        );
                    }
                });
        });
        if let Some(value) = selected {
            self.dismiss(Some(value));
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn placement(location: &str, area: egui::Rect, mouse: Pos2, size: egui::Vec2) -> Pos2 {
    let center = area.center() - size / 2.0;
    let result = match location {
        "WithMouse1" => mouse - size / 2.0,
        "WithMouse2" => mouse + egui::vec2(8.0, 8.0),
        "CenterScreen" => center,
        "TopLeft" => area.min,
        "TopCenter" => egui::pos2(center.x, area.top()),
        "TopRight" => egui::pos2(area.right() - size.x, area.top()),
        "LeftCenter" => egui::pos2(area.left(), center.y),
        "RightCenter" => egui::pos2(area.right() - size.x, center.y),
        "BottomLeft" => egui::pos2(area.left(), area.bottom() - size.y),
        "BottomCenter" => egui::pos2(center.x, area.bottom() - size.y),
        _ => area.max - size,
    };
    result.clamp(area.min, (area.max - size).max(area.min))
}
