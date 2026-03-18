use crate::action::{
    self, Action, ActionKind, ExecResult, PendingPluginRun, PluginRunOutcome, TextPluginStep,
};
use crate::config::Config;
use crate::focus::{self, FocusTracker};
use crate::search::SearchEngine;
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(800);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const RADIAL_MAX_INNER_ITEMS: usize = 8;
const RADIAL_CENTER_RADIUS: f32 = 34.0;
const RADIAL_INNER_RADIUS: f32 = 108.0;
const RADIAL_OUTER_RADIUS: f32 = 168.0;
const RADIAL_SELECTION_PADDING: f32 = 28.0;
const RADIAL_OVERLAY_MARGIN: f32 = RADIAL_OUTER_RADIUS + 8.0;
const EXTERNAL_ACTION_DELAY: Duration = Duration::from_millis(80);

#[derive(Clone)]
struct RadialMenuEntry {
    profile_idx: usize,
    section: ActionSection,
    action_idx: usize,
    action: Action,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionSection {
    GlobalTools,
    ActiveWindowTools,
}

impl ActionSection {
    fn label(self) -> &'static str {
        match self {
            Self::GlobalTools => "Global Tools",
            Self::ActiveWindowTools => "Current Window Tools",
        }
    }
}

#[derive(Clone)]
struct ActionScope {
    profile_idx: usize,
    section: ActionSection,
    path: Vec<usize>,
}

#[derive(Clone)]
struct ActionListEntry {
    profile_idx: usize,
    section: ActionSection,
    action_idx: usize,
    action: Action,
}

struct RadialMenuState {
    origin: egui::Pos2,
    pointer: egui::Pos2,
    entries: Vec<RadialMenuEntry>,
}

struct PluginPromptState {
    action_name: String,
    title: String,
    input: String,
    pending: PendingPluginRun,
}

enum DeferredExecution {
    Action(Action),
    PluginResume {
        action_name: String,
        pending: PendingPluginRun,
    },
}

struct DeferredExternalExecution {
    execute_at: Instant,
    execution: DeferredExecution,
}

/// Notification that disappears after a timeout.
struct Toast {
    message: String,
    is_error: bool,
    expires: std::time::Instant,
}

/// Which screen/view is active.
#[derive(Default, PartialEq)]
enum View {
    #[default]
    Panel,
    Settings,
    ActionEditor,
    ScriptOutput,
}

pub struct QuickerApp {
    config: Config,
    search: SearchEngine,
    _hotkey_manager: Option<GlobalHotKeyManager>,
    toggle_hotkey: Option<HotKey>,
    query: String,
    active_profile: usize,
    focus_tracker: FocusTracker,
    last_focus_poll: Instant,
    needs_focus_profile_sync: bool,
    action_scope: Option<ActionScope>,
    view: View,
    toast: Option<Toast>,
    script_output: String,
    radial_menu: Option<RadialMenuState>,
    panel_hidden: bool,
    startup_notice: Option<(String, bool)>,

    // Action editor state
    edit_name: String,
    edit_desc: String,
    edit_icon: String,
    edit_tags: String,
    edit_kind_idx: usize,
    edit_field1: String, // command / path / url / script / text / template
    edit_field2: String, // args / shell / fallback url
    edit_field3: String, // working_dir
    edit_plugin_steps: Vec<TextPluginStep>,
    plugin_prompt: Option<PluginPromptState>,
    deferred_external_execution: Option<DeferredExternalExecution>,
}

impl QuickerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        install_cjk_font_fallbacks(&cc.egui_ctx);

        // Style: make it look clean
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
        cc.egui_ctx.set_style(style);

        let (hotkey_manager, toggle_hotkey, startup_notice) =
            init_toggle_hotkey(&config.toggle_hotkey);

        Self {
            config,
            search: SearchEngine::new(),
            _hotkey_manager: hotkey_manager,
            toggle_hotkey,
            query: String::new(),
            active_profile: 0,
            focus_tracker: FocusTracker::new(std::process::id()),
            last_focus_poll: Instant::now() - FOCUS_POLL_INTERVAL,
            needs_focus_profile_sync: true,
            action_scope: None,
            view: View::Panel,
            toast: None,
            script_output: String::new(),
            radial_menu: None,
            panel_hidden: false,
            startup_notice,
            edit_name: String::new(),
            edit_desc: String::new(),
            edit_icon: String::new(),
            edit_tags: String::new(),
            edit_kind_idx: 0,
            edit_field1: String::new(),
            edit_field2: String::new(),
            edit_field3: String::new(),
            edit_plugin_steps: Vec::new(),
            plugin_prompt: None,
            deferred_external_execution: None,
        }
    }

    fn show_toast(&mut self, msg: String, is_error: bool) {
        self.toast = Some(Toast {
            message: msg,
            is_error,
            expires: std::time::Instant::now() + std::time::Duration::from_secs(3),
        });
    }

    fn handle_exec_result(&mut self, action_name: &str, result: ExecResult) {
        match result {
            ExecResult::Ok => self.show_toast(format!("✓ {}", action_name), false),
            ExecResult::OkWithMessage(msg) => {
                if msg.len() > 100 {
                    self.script_output = msg;
                    self.view = View::ScriptOutput;
                } else {
                    self.show_toast(msg, false);
                }
            }
            ExecResult::Err(err) => self.show_toast(err, true),
        }
    }

    fn reveal_panel(&mut self, ctx: &egui::Context) {
        if self.panel_hidden {
            self.panel_hidden = false;
            self.restore_panel_window(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn action_needs_external_focus(action: &Action) -> bool {
        match &action.kind {
            ActionKind::PluginPipeline { steps } => steps
                .iter()
                .any(|step| matches!(step, TextPluginStep::WriteSelectedText)),
            _ => false,
        }
    }

    fn pending_run_needs_external_focus(pending: &PendingPluginRun) -> bool {
        pending.steps[pending.next_step_idx..]
            .iter()
            .any(|step| matches!(step, TextPluginStep::WriteSelectedText))
    }

    fn defer_external_execution(&mut self, ctx: &egui::Context, execution: DeferredExecution) {
        self.deferred_external_execution = Some(DeferredExternalExecution {
            execute_at: Instant::now() + EXTERNAL_ACTION_DELAY,
            execution,
        });
        self.panel_hidden = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn run_deferred_external_execution(&mut self, ctx: &egui::Context) {
        let Some(deferred) = self.deferred_external_execution.take() else {
            return;
        };

        match deferred.execution {
            DeferredExecution::Action(action) => {
                let outcome = match &action.kind {
                    ActionKind::PluginPipeline { steps } => action::run_plugin_pipeline(steps),
                    _ => {
                        let result = action.execute();
                        self.reveal_panel(ctx);
                        self.handle_exec_result(&action.name, result);
                        return;
                    }
                };

                self.reveal_panel(ctx);
                match outcome {
                    PluginRunOutcome::Complete(result) => {
                        self.handle_exec_result(&action.name, result)
                    }
                    PluginRunOutcome::NeedsInput { prompt, pending } => {
                        self.plugin_prompt = Some(PluginPromptState {
                            action_name: action.name.clone(),
                            title: prompt.title,
                            input: prompt.value,
                            pending,
                        });
                    }
                }
            }
            DeferredExecution::PluginResume {
                action_name,
                pending,
            } => {
                let outcome = action::continue_plugin_pipeline(pending);
                self.reveal_panel(ctx);
                match outcome {
                    PluginRunOutcome::Complete(result) => {
                        self.handle_exec_result(&action_name, result)
                    }
                    PluginRunOutcome::NeedsInput { prompt, pending } => {
                        self.plugin_prompt = Some(PluginPromptState {
                            action_name,
                            title: prompt.title,
                            input: prompt.value,
                            pending,
                        });
                    }
                }
            }
        }
    }

    fn run_plugin_pipeline(&mut self, action_name: &str, pending: PendingPluginRun) {
        match action::continue_plugin_pipeline(pending) {
            PluginRunOutcome::Complete(result) => self.handle_exec_result(action_name, result),
            PluginRunOutcome::NeedsInput { prompt, pending } => {
                self.plugin_prompt = Some(PluginPromptState {
                    action_name: action_name.into(),
                    title: prompt.title,
                    input: prompt.value,
                    pending,
                });
            }
        }
    }

    fn execute_action(&mut self, ctx: &egui::Context, action: &Action) {
        if Self::action_needs_external_focus(action) {
            self.defer_external_execution(ctx, DeferredExecution::Action(action.clone()));
            return;
        }

        match &action.kind {
            ActionKind::PluginPipeline { steps } => match action::run_plugin_pipeline(steps) {
                PluginRunOutcome::Complete(result) => self.handle_exec_result(&action.name, result),
                PluginRunOutcome::NeedsInput { prompt, pending } => {
                    self.plugin_prompt = Some(PluginPromptState {
                        action_name: action.name.clone(),
                        title: prompt.title,
                        input: prompt.value,
                        pending,
                    });
                }
            },
            _ => self.handle_exec_result(&action.name, action.execute()),
        }
    }

    fn trigger_action(
        &mut self,
        ctx: &egui::Context,
        profile_idx: usize,
        section: ActionSection,
        action_idx: usize,
        action: Action,
    ) {
        match action.kind {
            ActionKind::Group { .. } => self.open_group(profile_idx, section, action_idx),
            _ => self.execute_action(ctx, &action),
        }
    }

    fn set_active_profile(&mut self, profile_idx: usize) {
        if self.active_profile != profile_idx {
            self.active_profile = profile_idx;
            self.query.clear();
            if matches!(
                self.action_scope.as_ref(),
                Some(scope)
                    if scope.section == ActionSection::ActiveWindowTools
                        && scope.profile_idx != profile_idx
            ) {
                self.action_scope = None;
            }
        }
    }

    fn poll_focused_process(&mut self) {
        if self.last_focus_poll.elapsed() < FOCUS_POLL_INTERVAL {
            return;
        }

        self.last_focus_poll = Instant::now();
        if self.focus_tracker.observe(focus::detect_focused_process()) {
            self.needs_focus_profile_sync = true;
        }
    }

    fn sync_profile_to_focus(&mut self) {
        let Some(process) = self.focus_tracker.current_external().cloned() else {
            return;
        };

        self.sync_profile_to_process(&process);
    }

    fn sync_profile_to_process(&mut self, process: &focus::FocusedProcess) {
        let profile_idx = self
            .config
            .matching_profile_index(process)
            .unwrap_or(self.global_profile_idx());
        self.set_active_profile(profile_idx);
    }

    fn reset_editor(&mut self) {
        self.edit_name.clear();
        self.edit_desc.clear();
        self.edit_icon.clear();
        self.edit_tags.clear();
        self.edit_kind_idx = 0;
        self.edit_field1.clear();
        self.edit_field2.clear();
        self.edit_field3.clear();
        self.edit_plugin_steps.clear();
    }

    fn global_profile_idx(&self) -> usize {
        0
    }

    fn active_window_profile_index(&self) -> Option<usize> {
        (self.active_profile != self.global_profile_idx()).then_some(self.active_profile)
    }

    fn profile_actions(&self, profile_idx: usize) -> &[Action] {
        self.config
            .profiles
            .get(profile_idx)
            .map(|p| p.actions.as_slice())
            .unwrap_or(&[])
    }

    fn actions_at_path<'a>(actions: &'a [Action], path: &[usize]) -> Option<&'a [Action]> {
        let mut current = actions;
        for &idx in path {
            let action = current.get(idx)?;
            match &action.kind {
                ActionKind::Group { actions } => current = actions,
                _ => return None,
            }
        }
        Some(current)
    }

    fn actions_at_path_mut<'a>(
        actions: &'a mut Vec<Action>,
        path: &[usize],
    ) -> Option<&'a mut Vec<Action>> {
        if let Some((&idx, rest)) = path.split_first() {
            let action = actions.get_mut(idx)?;
            match &mut action.kind {
                ActionKind::Group { actions } => Self::actions_at_path_mut(actions, rest),
                _ => None,
            }
        } else {
            Some(actions)
        }
    }

    fn actions_for_scope<'a>(&'a self, scope: &'a ActionScope) -> &'a [Action] {
        Self::actions_at_path(self.profile_actions(scope.profile_idx), &scope.path)
            .unwrap_or_else(|| self.profile_actions(scope.profile_idx))
    }

    fn action_entries(
        &self,
        profile_idx: usize,
        section: ActionSection,
        actions: &[Action],
    ) -> Vec<ActionListEntry> {
        actions
            .iter()
            .cloned()
            .enumerate()
            .map(|(action_idx, action)| ActionListEntry {
                profile_idx,
                section,
                action_idx,
                action,
            })
            .collect()
    }

    fn current_action_entries(&self) -> Vec<ActionListEntry> {
        if let Some(scope) = &self.action_scope {
            return self.action_entries(
                scope.profile_idx,
                scope.section,
                self.actions_for_scope(scope),
            );
        }

        let mut entries = self.action_entries(
            self.global_profile_idx(),
            ActionSection::GlobalTools,
            self.profile_actions(self.global_profile_idx()),
        );

        if let Some(profile_idx) = self.active_window_profile_index() {
            entries.extend(self.action_entries(
                profile_idx,
                ActionSection::ActiveWindowTools,
                self.profile_actions(profile_idx),
            ));
        }

        entries
    }

    fn current_actions_mut(&mut self) -> Option<&mut Vec<Action>> {
        let (profile_idx, path) = if let Some(scope) = &self.action_scope {
            (scope.profile_idx, scope.path.clone())
        } else {
            (
                self.active_window_profile_index()
                    .unwrap_or(self.global_profile_idx()),
                Vec::new(),
            )
        };

        let profile = self.config.profiles.get_mut(profile_idx)?;
        Self::actions_at_path_mut(&mut profile.actions, &path)
    }

    fn group_titles(&self) -> Vec<String> {
        let Some(scope) = &self.action_scope else {
            return Vec::new();
        };

        let mut titles = Vec::new();
        let mut current = self.profile_actions(scope.profile_idx);
        for &idx in &scope.path {
            let Some(action) = current.get(idx) else {
                break;
            };
            titles.push(action.name.clone());
            match &action.kind {
                ActionKind::Group { actions } => current = actions,
                _ => break,
            }
        }
        titles
    }

    fn open_group(&mut self, profile_idx: usize, section: ActionSection, action_idx: usize) {
        match &mut self.action_scope {
            Some(scope) if scope.profile_idx == profile_idx && scope.section == section => {
                scope.path.push(action_idx);
            }
            _ => {
                self.action_scope = Some(ActionScope {
                    profile_idx,
                    section,
                    path: vec![action_idx],
                });
            }
        }
        self.query.clear();
    }

    fn leave_group(&mut self) {
        let Some(scope) = &mut self.action_scope else {
            return;
        };

        scope.path.pop();
        if scope.path.is_empty() {
            self.action_scope = None;
        }
        self.query.clear();
    }

    fn radial_entries(&self) -> Vec<RadialMenuEntry> {
        let entries = self.current_action_entries();
        let results = self.filtered_entries(&entries);
        let entries = if results.is_empty() { entries } else { results };

        entries
            .into_iter()
            .map(|entry| RadialMenuEntry {
                profile_idx: entry.profile_idx,
                section: entry.section,
                action_idx: entry.action_idx,
                action: entry.action,
            })
            .collect()
    }

    fn start_local_radial_menu(&mut self, ctx: &egui::Context, pointer: egui::Pos2) {
        let entries = self.radial_entries();
        if entries.is_empty() {
            return;
        }

        let screen = ctx.content_rect();
        let origin = egui::pos2(
            clamp_to_view(
                pointer.x,
                screen.left() + RADIAL_OVERLAY_MARGIN,
                screen.right() - RADIAL_OVERLAY_MARGIN,
            ),
            clamp_to_view(
                pointer.y,
                screen.top() + RADIAL_OVERLAY_MARGIN,
                screen.bottom() - RADIAL_OVERLAY_MARGIN,
            ),
        );

        self.radial_menu = Some(RadialMenuState {
            origin,
            pointer,
            entries,
        });
        ctx.request_repaint();
    }

    fn restore_panel_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::viewport::WindowLevel::Normal,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            self.config.panel_width,
            self.config.panel_height,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(!self.panel_hidden));
    }

    fn complete_radial_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.radial_menu.take() else {
            return;
        };

        if let Some(entry_idx) = radial_hovered_entry(
            menu.origin,
            menu.pointer,
            radial_ring_counts(menu.entries.len()),
        ) {
            let entry = menu.entries[entry_idx].clone();
            self.trigger_action(
                ctx,
                entry.profile_idx,
                entry.section,
                entry.action_idx,
                entry.action,
            );
        }

        let _ = ctx;
    }

    fn cancel_radial_menu(&mut self, ctx: &egui::Context) {
        let Some(_menu) = self.radial_menu.take() else {
            return;
        };
        let _ = ctx;
    }

    fn handle_radial_menu_input(&mut self, ctx: &egui::Context) {
        if self.view != View::Panel {
            return;
        }

        let (pointer_pos, secondary_pressed, secondary_down, secondary_released) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.button_pressed(egui::PointerButton::Secondary),
                i.pointer.button_down(egui::PointerButton::Secondary),
                i.pointer.button_released(egui::PointerButton::Secondary),
            )
        });

        if secondary_pressed {
            if let Some(pointer) = pointer_pos {
                self.start_local_radial_menu(ctx, pointer);
            }
        }

        if let Some(menu) = &mut self.radial_menu {
            if let Some(pointer) = pointer_pos {
                menu.pointer = pointer;
            }
        }

        if secondary_released {
            self.complete_radial_menu(ctx);
        } else if self.radial_menu.is_some() && secondary_down {
            ctx.request_repaint();
        }
    }

    fn filtered_entries(&self, entries: &[ActionListEntry]) -> Vec<ActionListEntry> {
        let actions: Vec<Action> = entries.iter().map(|entry| entry.action.clone()).collect();
        self.search
            .search(&self.query, &actions)
            .into_iter()
            .map(|(_, idx, _)| entries[idx].clone())
            .collect()
    }

    fn add_action_target_profile_idx(&self) -> usize {
        self.action_scope
            .as_ref()
            .map(|scope| scope.profile_idx)
            .or_else(|| self.active_window_profile_index())
            .unwrap_or(self.global_profile_idx())
    }

    fn add_action_target_label(&self) -> String {
        let profile_name = self
            .config
            .profiles
            .get(self.add_action_target_profile_idx())
            .map(|profile| profile.name.as_str())
            .unwrap_or("Default");

        if let Some(scope) = &self.action_scope {
            format!("{} / {}", scope.section.label(), profile_name)
        } else if self.active_window_profile_index().is_some() {
            format!(
                "{} / {}",
                ActionSection::ActiveWindowTools.label(),
                profile_name
            )
        } else {
            format!("{} / {}", ActionSection::GlobalTools.label(), profile_name)
        }
    }

    fn render_plugin_steps_editor(&mut self, ui: &mut egui::Ui) {
        ui.label("Plugin commands:");
        ui.label(
            egui::RichText::new(
                "This plugin currently supports 6 commands. Drag rows to reorder how the plugin runs.",
            )
            .weak()
            .small(),
        );
        ui.add_space(6.0);

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(egui::RichText::new("Selection").strong());
                ui.label(
                    egui::RichText::new("Read text from the selected part.")
                        .small()
                        .weak(),
                );
                if ui.button("Read Selected Text").clicked() {
                    self.edit_plugin_steps
                        .push(TextPluginStep::ReadSelectedText);
                }
                if ui.button("Read Clipboard Text").clicked() {
                    self.edit_plugin_steps
                        .push(TextPluginStep::ReadClipboardText);
                }
            });

            columns[1].group(|ui| {
                ui.label(egui::RichText::new("Transform").strong());
                ui.label(
                    egui::RichText::new("Modify text inside the plugin flow.")
                        .small()
                        .weak(),
                );
                if ui.button("Uppercase Text").clicked() {
                    self.edit_plugin_steps.push(TextPluginStep::Uppercase);
                }
            });
        });

        ui.add_space(6.0);

        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(egui::RichText::new("Output").strong());
                ui.label(
                    egui::RichText::new("Write the result to a selection or clipboard.")
                        .small()
                        .weak(),
                );
                if ui.button("Write Selected Text").clicked() {
                    self.edit_plugin_steps
                        .push(TextPluginStep::WriteSelectedText);
                }
                if ui.button("Write Clipboard Text").clicked() {
                    self.edit_plugin_steps
                        .push(TextPluginStep::WriteClipboardText);
                }
            });

            columns[1].group(|ui| {
                ui.label(egui::RichText::new("Interaction").strong());
                ui.label(
                    egui::RichText::new("Ask the user to enter text.")
                        .small()
                        .weak(),
                );
                if ui.button("Prompt For Input").clicked() {
                    self.edit_plugin_steps.push(TextPluginStep::PromptInput {
                        title: "Input".into(),
                        default_value: String::new(),
                    });
                }
            });
        });

        ui.add_space(8.0);

        if self.edit_plugin_steps.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Add at least one command. Examples: Read Selected Text -> Uppercase Text -> Write Selected Text, or Read Clipboard Text -> Uppercase Text -> Write Clipboard Text.",
                )
                .weak(),
            );
            return;
        }

        let mut move_request = None;
        let mut delete_request = None;
        let len = self.edit_plugin_steps.len();

        for idx in 0..len {
            let id = egui::Id::new(("plugin_step", idx));
            let frame = egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(12, 10))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(70)));
            let (inner, dropped) = ui.dnd_drop_zone::<usize, _>(frame, |ui| {
                ui.dnd_drag_source(id, idx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⋮⋮").monospace());
                        ui.label(
                            egui::RichText::new(format!(
                                "{}. {}",
                                idx + 1,
                                self.edit_plugin_steps[idx].label()
                            ))
                            .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Delete").clicked() {
                                delete_request = Some(idx);
                            }
                        });
                    });

                    if let TextPluginStep::PromptInput {
                        title,
                        default_value,
                    } = &mut self.edit_plugin_steps[idx]
                    {
                        ui.add_space(6.0);
                        ui.label("Dialog title:");
                        ui.text_edit_singleline(title);
                        ui.label("Default value:");
                        ui.text_edit_singleline(default_value);
                    }
                });
            });

            if inner.response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }

            if let Some(from_idx) = dropped.as_deref().copied() {
                move_request = Some((from_idx, idx));
            }

            ui.add_space(6.0);
        }

        let end_frame = egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::symmetric(12, 10))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(55)));
        let (_, dropped_at_end) = ui.dnd_drop_zone::<usize, _>(end_frame, |ui| {
            ui.label(egui::RichText::new("Drop here to move to the end").weak());
        });

        if let Some(from_idx) = dropped_at_end.as_deref().copied() {
            move_request = Some((from_idx, self.edit_plugin_steps.len()));
        }

        if let Some(idx) = delete_request {
            self.edit_plugin_steps.remove(idx);
        }

        if let Some((from_idx, to_idx)) = move_request {
            move_list_item(&mut self.edit_plugin_steps, from_idx, to_idx);
        }
    }

    fn render_plugin_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.plugin_prompt else {
            return;
        };

        let mut submit = false;
        let mut cancel = false;

        egui::Window::new(prompt.title.clone())
            .id(egui::Id::new("plugin_prompt_window"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Enter text for this plugin step:");
                ui.label(
                    egui::RichText::new(
                        "This command needs manual input before the plugin can continue.",
                    )
                    .small()
                    .weak(),
                );
                let response =
                    ui.add(egui::TextEdit::singleline(&mut prompt.input).desired_width(320.0));
                if ui.memory(|memory| memory.focused().is_none()) {
                    response.request_focus();
                }

                submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("OK").clicked() {
                        submit = true;
                    }
                });
            });

        if cancel {
            self.plugin_prompt = None;
            self.show_toast("Plugin input cancelled".into(), true);
            return;
        }

        if submit {
            if let Some(mut prompt) = self.plugin_prompt.take() {
                prompt.pending.current_text = prompt.input;
                if Self::pending_run_needs_external_focus(&prompt.pending) {
                    self.defer_external_execution(
                        ctx,
                        DeferredExecution::PluginResume {
                            action_name: prompt.action_name,
                            pending: prompt.pending,
                        },
                    );
                } else {
                    self.run_plugin_pipeline(&prompt.action_name, prompt.pending);
                }
            }
        }
    }

    fn render_action_results_grid(
        &mut self,
        ui: &mut egui::Ui,
        grid_id: &str,
        entries: &[ActionListEntry],
    ) {
        let cols = self.config.columns;
        let mut clicked_action: Option<ActionListEntry> = None;

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

                    let btn = egui::Button::new(egui::RichText::new(&label).size(14.0))
                        .min_size(egui::vec2(btn_width, 48.0));

                    let response = ui.add(btn);

                    if !entry.action.description.is_empty() {
                        response.clone().on_hover_text(&entry.action.description);
                    }

                    if response.clicked() {
                        clicked_action = Some(entry.clone());
                    }
                }
            });

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

    fn render_panel(&mut self, ui: &mut egui::Ui) {
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
                if ui.button("＋").on_hover_text("Add action").clicked() {
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

        // --- Search bar ---
        let search_response = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("🔍 Search actions...")
                .desired_width(f32::INFINITY),
        );
        // Auto-focus on startup
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
            self.profile_actions(global_profile_idx),
        ));

        let active_window_entries = self
            .active_window_profile_index()
            .map(|profile_idx| {
                self.filtered_entries(&self.action_entries(
                    profile_idx,
                    ActionSection::ActiveWindowTools,
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

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            }
            ui.heading("Settings");
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .show(ui, |ui| {
            let current_process = self.focus_tracker.current_external().cloned();
            let current_process_alias = current_process.as_ref().map(|process| process.primary_alias());
            let current_process_label = current_process
                .as_ref()
                .map(|process| process.display_name())
                .unwrap_or_else(|| "Unavailable".into());
            let mut profile_rules_changed = false;

            ui.label("Toggle Hotkey:");
            ui.text_edit_singleline(&mut self.config.toggle_hotkey);
            ui.add_space(8.0);

            ui.label("Grid Columns:");
            ui.add(egui::Slider::new(&mut self.config.columns, 2..=8));
            ui.add_space(8.0);

            ui.label("Panel Width:");
            ui.add(egui::Slider::new(
                &mut self.config.panel_width,
                300.0..=1200.0,
            ));

            ui.label("Panel Height:");
            ui.add(egui::Slider::new(
                &mut self.config.panel_height,
                200.0..=900.0,
            ));

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);

            ui.heading("Profiles");
            ui.label(format!("Current focused process: {}", current_process_label));
            ui.label(
                egui::RichText::new(
                    "The first profile is fixed as the upper Global Tools section. Profiles with match rules are shown in the lower Current Window Tools section when their app is focused.",
                )
                .weak()
                .small(),
            );
            ui.add_space(8.0);

            let mut to_delete: Option<usize> = None;
            let can_delete_profiles = self.config.profiles.len() > 1;
            for (i, profile) in self.config.profiles.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut profile.name);
                    ui.label(format!("({} actions)", profile.actions.len()));
                    if i > 0 {
                        if let Some(alias) = current_process_alias.as_deref() {
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
                    if can_delete_profiles && i > 0 {
                        if ui.small_button("🗑").clicked() {
                            to_delete = Some(i);
                        }
                    }
                });
                if i == 0 {
                    ui.label(
                        egui::RichText::new(
                            "This profile is always shown in the top half and does not use match rules.",
                        )
                        .weak()
                        .small(),
                    );
                } else {
                    let mut process_matches = profile.match_processes.join(", ");
                    ui.label("Match focused processes (comma-separated):");
                    if ui.text_edit_singleline(&mut process_matches).changed() {
                        profile.match_processes = process_matches
                            .split(',')
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string)
                            .collect();
                        profile_rules_changed = true;
                    }
                }
                ui.add_space(8.0);
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

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button("💾 Save Config").clicked() {
                    self.config.save();
                    self.show_toast("Config saved!".into(), false);
                    self.needs_focus_profile_sync = true;
                }
                ui.label(
                    egui::RichText::new(format!("Config: {}", Config::config_path().display()))
                        .weak()
                        .small(),
                );
            });
            });
    }

    fn render_action_editor(&mut self, ui: &mut egui::Ui) {
        let is_plugin_editor = self.edit_kind_idx == 10;

        ui.horizontal(|ui| {
            if ui.button("← Cancel").clicked() {
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            }
            ui.heading(if is_plugin_editor {
                "Add Plugin"
            } else {
                "Add Action"
            });
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("action_editor_scroll")
            .show(ui, |ui| {
            ui.label(
                egui::RichText::new(format!("Adding into: {}", self.add_action_target_label()))
                    .weak()
                    .small(),
            );
            ui.add_space(8.0);

            ui.label("Name:");
            ui.text_edit_singleline(&mut self.edit_name);

            ui.label("Description:");
            ui.text_edit_singleline(&mut self.edit_desc);

            ui.label("Icon (emoji):");
            ui.text_edit_singleline(&mut self.edit_icon);

            ui.label("Tags (comma-separated):");
            ui.text_edit_singleline(&mut self.edit_tags);

            ui.add_space(8.0);
            ui.label(if is_plugin_editor { "Item Type:" } else { "Action Type:" });

            let kinds = [
                "Run Program",
                "Open File",
                "Open URL",
                "Run Shell Script",
                "Copy Text",
                "Open Folder",
                "Group",
                "Search Clipboard Text",
                "Open Clipboard Text",
                "Run Clipboard Text",
                "Plugin",
            ];
            egui::ComboBox::from_id_salt("action_kind")
                .selected_text(kinds[self.edit_kind_idx])
                .show_ui(ui, |ui| {
                    for (i, kind) in kinds.iter().enumerate() {
                        ui.selectable_value(&mut self.edit_kind_idx, i, *kind);
                    }
                });

            ui.add_space(8.0);

            match self.edit_kind_idx {
                0 => {
                    ui.label("Command / executable path:");
                    ui.text_edit_singleline(&mut self.edit_field1);
                    ui.label("Arguments (space-separated):");
                    ui.text_edit_singleline(&mut self.edit_field2);
                    ui.label("Working directory (optional):");
                    ui.text_edit_singleline(&mut self.edit_field3);
                }
                1 => {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.edit_field1);
                }
                2 => {
                    ui.label("URL:");
                    ui.text_edit_singleline(&mut self.edit_field1);
                }
                3 => {
                    ui.label("Shell (sh, bash, powershell, cmd):");
                    ui.text_edit_singleline(&mut self.edit_field2);
                    ui.label("Script:");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.edit_field1)
                            .desired_width(f32::INFINITY)
                            .desired_rows(6)
                            .code_editor(),
                    );
                }
                4 => {
                    ui.label("Text to copy:");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.edit_field1)
                            .desired_width(f32::INFINITY)
                            .desired_rows(4),
                    );
                }
                5 => {
                    ui.label("Folder path:");
                    ui.text_edit_singleline(&mut self.edit_field1);
                }
                6 => {
                    ui.label("Creates an empty group. Open it from the panel and add child actions inside it.");
                }
                7 => {
                    ui.label("Search URL template:");
                    ui.text_edit_singleline(&mut self.edit_field1);
                    ui.label("Use {query} where clipboard text should be inserted.");
                }
                8 => {
                    ui.label("Fallback search URL template (optional):");
                    ui.text_edit_singleline(&mut self.edit_field2);
                    ui.label("If clipboard is not a URL/path, use {query} in this template to search it.");
                }
                9 => {
                    ui.label("Shell (sh, bash, powershell, cmd):");
                    ui.text_edit_singleline(&mut self.edit_field2);
                    ui.label("Runs the current clipboard text as a command.");
                }
                10 => {
                    self.render_plugin_steps_editor(ui);
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Commands are organized by functionality: Selection, Transform, Output, and Interaction.",
                        )
                        .weak()
                        .small(),
                    );
                }
                _ => {}
            }

            ui.add_space(16.0);

            if ui.button("✓ Save Action").clicked() && !self.edit_name.is_empty() {
                let kind = match self.edit_kind_idx {
                    0 => ActionKind::RunProgram {
                        command: self.edit_field1.clone(),
                        args: self
                            .edit_field2
                            .split_whitespace()
                            .map(String::from)
                            .collect(),
                        working_dir: if self.edit_field3.is_empty() {
                            None
                        } else {
                            Some(self.edit_field3.clone())
                        },
                    },
                    1 => ActionKind::OpenFile {
                        path: self.edit_field1.clone(),
                    },
                    2 => ActionKind::OpenUrl {
                        url: self.edit_field1.clone(),
                    },
                    3 => ActionKind::RunShell {
                        script: self.edit_field1.clone(),
                        shell: if self.edit_field2.is_empty() {
                            "sh".into()
                        } else {
                            self.edit_field2.clone()
                        },
                    },
                    4 => ActionKind::CopyText {
                        text: self.edit_field1.clone(),
                    },
                    5 => ActionKind::OpenFolder {
                        path: self.edit_field1.clone(),
                    },
                    6 => ActionKind::Group { actions: vec![] },
                    7 => ActionKind::SearchClipboardText {
                        url_template: if self.edit_field1.is_empty() {
                            "https://www.google.com/search?q={query}".into()
                        } else {
                            self.edit_field1.clone()
                        },
                    },
                    8 => ActionKind::OpenClipboardText {
                        fallback_search_url: if self.edit_field2.is_empty() {
                            Some("https://www.google.com/search?q={query}".into())
                        } else {
                            Some(self.edit_field2.clone())
                        },
                    },
                    9 => ActionKind::RunClipboardText {
                        shell: if self.edit_field2.is_empty() {
                            "sh".into()
                        } else {
                            self.edit_field2.clone()
                        },
                    },
                    10 => {
                        if self.edit_plugin_steps.is_empty() {
                            self.show_toast(
                                "Add at least one plugin step before saving.".into(),
                                true,
                            );
                            return;
                        }
                        ActionKind::PluginPipeline {
                            steps: self.edit_plugin_steps.clone(),
                        }
                    }
                    _ => unreachable!(),
                };

                let action = Action {
                    name: self.edit_name.clone(),
                    description: self.edit_desc.clone(),
                    icon: if self.edit_icon.is_empty() {
                        None
                    } else {
                        Some(self.edit_icon.clone())
                    },
                    tags: self
                        .edit_tags
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    hotkey: None,
                    kind,
                };

                if let Some(actions) = self.current_actions_mut() {
                    actions.push(action);
                }
                self.config.save();
                self.show_toast("Action added!".into(), false);
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            }
            });
    }

    fn render_script_output(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            }
            ui.heading("Script Output");
            if ui.button("📋 Copy").clicked() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(&self.script_output);
                    self.show_toast("Copied!".into(), false);
                }
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

    fn render_toast(&mut self, ctx: &egui::Context) {
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

    fn handle_global_hotkey(&mut self, ctx: &egui::Context) {
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

    fn show_startup_notice_once(&mut self) {
        if let Some((message, is_error)) = self.startup_notice.take() {
            self.show_toast(message, is_error);
        }
    }

    fn render_radial_menu(&self, ctx: &egui::Context) {
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

impl eframe::App for QuickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show_startup_notice_once();
        self.handle_global_hotkey(ctx);

        // Handle Escape key
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.radial_menu.is_some() {
                self.cancel_radial_menu(ctx);
                ctx.request_repaint();
            } else if self.plugin_prompt.is_some() {
                self.plugin_prompt = None;
                self.show_toast("Plugin input cancelled".into(), true);
            } else if self.view != View::Panel {
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            } else if self.action_scope.is_some() {
                self.leave_group();
            }
        }

        self.poll_focused_process();
        if self.view == View::Panel && self.needs_focus_profile_sync {
            self.sync_profile_to_focus();
            self.needs_focus_profile_sync = false;
        }
        if self
            .deferred_external_execution
            .as_ref()
            .is_some_and(|deferred| Instant::now() >= deferred.execute_at)
        {
            self.run_deferred_external_execution(ctx);
        }
        self.handle_radial_menu_input(ctx);

        if !self.panel_hidden {
            egui::CentralPanel::default().show(ctx, |ui| match self.view {
                View::Panel => self.render_panel(ui),
                View::Settings => self.render_settings(ui),
                View::ActionEditor => self.render_action_editor(ui),
                View::ScriptOutput => self.render_script_output(ui),
            });
        }

        self.render_plugin_prompt(ctx);
        self.render_radial_menu(ctx);
        self.render_toast(ctx);
        ctx.request_repaint_after(INPUT_POLL_INTERVAL);
    }
}

fn default_action_icon(action: &Action) -> &'static str {
    match &action.kind {
        ActionKind::Group { .. } => "📂",
        ActionKind::PluginPipeline { .. } => "🧩",
        _ => "▶",
    }
}

fn clamp_to_view(value: f32, min: f32, max: f32) -> f32 {
    if min <= max {
        value.clamp(min, max)
    } else {
        (min + max) * 0.5
    }
}

fn move_list_item<T>(items: &mut Vec<T>, from_idx: usize, to_idx: usize) {
    if from_idx >= items.len() || from_idx == to_idx {
        return;
    }

    let item = items.remove(from_idx);
    let insert_idx = if from_idx < to_idx {
        to_idx.saturating_sub(1)
    } else {
        to_idx
    }
    .min(items.len());
    items.insert(insert_idx, item);
}

fn radial_ring_counts(entry_count: usize) -> (usize, usize) {
    if entry_count <= RADIAL_MAX_INNER_ITEMS {
        (entry_count, 0)
    } else {
        (RADIAL_MAX_INNER_ITEMS, entry_count - RADIAL_MAX_INNER_ITEMS)
    }
}

fn radial_hovered_entry(
    origin: egui::Pos2,
    pointer: egui::Pos2,
    (inner_count, outer_count): (usize, usize),
) -> Option<usize> {
    let delta = pointer - origin;
    let distance = delta.length();

    if distance <= RADIAL_CENTER_RADIUS {
        return None;
    }

    if outer_count > 0 && distance > RADIAL_INNER_RADIUS {
        if distance > RADIAL_OUTER_RADIUS + RADIAL_SELECTION_PADDING {
            return None;
        }
        radial_sector_index(delta, outer_count).map(|idx| inner_count + idx)
    } else {
        if distance > RADIAL_INNER_RADIUS + RADIAL_SELECTION_PADDING {
            return None;
        }
        radial_sector_index(delta, inner_count)
    }
}

fn radial_sector_index(delta: egui::Vec2, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }

    let angle = (delta.y.atan2(delta.x) + FRAC_PI_2).rem_euclid(TAU);
    let step = TAU / count as f32;
    Some((((angle + step * 0.5) / step).floor() as usize) % count)
}

fn paint_radial_ring(
    painter: &egui::Painter,
    center: egui::Pos2,
    entries: &[RadialMenuEntry],
    offset: usize,
    inner_radius: f32,
    outer_radius: f32,
    hovered: Option<usize>,
) {
    let count = entries.len();
    if count == 0 {
        return;
    }

    let step = TAU / count as f32;
    let base_text_size = if offset == 0 { 15.0 } else { 13.0 };

    for (slot, entry) in entries.iter().enumerate() {
        let start_angle = slot as f32 * step - step * 0.5 - FRAC_PI_2;
        let end_angle = start_angle + step;
        let is_hovered = hovered == Some(offset + slot);
        let fill = if is_hovered {
            egui::Color32::from_rgb(66, 133, 244)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 244)
        };
        let stroke = if is_hovered {
            egui::Stroke::new(2.0, egui::Color32::from_rgb(32, 87, 184))
        } else {
            egui::Stroke::new(1.0, egui::Color32::from_gray(210))
        };

        painter.add(egui::Shape::convex_polygon(
            radial_sector_points(center, inner_radius, outer_radius, start_angle, end_angle),
            fill,
            stroke,
        ));

        let label_radius = (inner_radius + outer_radius) * 0.5;
        let label_angle = start_angle + step * 0.5;
        let label_pos = center + egui::vec2(label_angle.cos(), label_angle.sin()) * label_radius;
        let icon = entry
            .action
            .icon
            .as_deref()
            .unwrap_or(default_action_icon(&entry.action));
        let label = format!(
            "{}\n{}",
            icon,
            truncate_label(&entry.action.name, if offset == 0 { 10 } else { 12 })
        );
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(base_text_size),
            if is_hovered {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(30)
            },
        );
    }
}

fn radial_sector_points(
    center: egui::Pos2,
    inner_radius: f32,
    outer_radius: f32,
    start_angle: f32,
    end_angle: f32,
) -> Vec<egui::Pos2> {
    let sweep = (end_angle - start_angle).abs();
    let arc_steps = ((sweep / 0.25).ceil() as usize).max(4);
    let mut points = Vec::with_capacity(arc_steps * 2 + 2);

    for step in 0..=arc_steps {
        let t = step as f32 / arc_steps as f32;
        let angle = egui::lerp(start_angle..=end_angle, t);
        points.push(center + egui::vec2(angle.cos(), angle.sin()) * outer_radius);
    }

    for step in (0..=arc_steps).rev() {
        let t = step as f32 / arc_steps as f32;
        let angle = egui::lerp(start_angle..=end_angle, t);
        points.push(center + egui::vec2(angle.cos(), angle.sin()) * inner_radius);
    }

    points
}

fn truncate_label(text: &str, max_chars: usize) -> String {
    let mut label = String::new();

    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            label.push_str("...");
            break;
        }
        label.push(ch);
    }

    label
}

fn install_cjk_font_fallbacks(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_fonts = Vec::new();

    for path in cjk_font_candidates() {
        let Some(font_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if fonts.font_data.contains_key(font_name) {
            continue;
        }

        match std::fs::read(&path) {
            Ok(data) => {
                let font_name = font_name.to_owned();
                fonts.font_data.insert(
                    font_name.clone(),
                    std::sync::Arc::new(egui::FontData::from_owned(data)),
                );

                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    family.push(font_name.clone());
                }
                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    family.push(font_name.clone());
                }

                loaded_fonts.push(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::debug!("failed to load font {}: {}", path.display(), err),
        }
    }

    if loaded_fonts.is_empty() {
        tracing::debug!("no CJK fallback fonts found on the system");
        return;
    }

    ctx.set_fonts(fonts);
    tracing::info!(
        "loaded CJK fallback fonts: {}",
        loaded_fonts
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn cjk_font_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    extend_if_exists(
        &mut candidates,
        dirs::home_dir(),
        &[
            ".local/share/fonts/NotoSansCJK-Regular.ttc",
            ".local/share/fonts/NotoSerifCJK-Regular.ttc",
            ".local/share/fonts/SourceHanSansSC-Regular.otf",
            ".local/share/fonts/SourceHanSansCN-Regular.otf",
            ".local/share/fonts/SourceHanSansJP-Regular.otf",
            ".local/share/fonts/SourceHanSansKR-Regular.otf",
            ".local/share/fonts/SourceHanSansTW-Regular.otf",
            ".local/share/fonts/NanumGothic.ttf",
            ".local/share/fonts/wqy-zenhei.ttc",
            ".fonts/NotoSansCJK-Regular.ttc",
            ".fonts/NotoSerifCJK-Regular.ttc",
            ".fonts/SourceHanSansSC-Regular.otf",
            ".fonts/SourceHanSansCN-Regular.otf",
            ".fonts/SourceHanSansJP-Regular.otf",
            ".fonts/SourceHanSansKR-Regular.otf",
            ".fonts/SourceHanSansTW-Regular.otf",
            ".fonts/NanumGothic.ttf",
            ".fonts/wqy-zenhei.ttc",
        ],
    );

    #[cfg(target_os = "linux")]
    extend_if_exists(
        &mut candidates,
        None,
        &[
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansJP-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansKR-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansSC-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansTC-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansSC-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansCN-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansJP-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansKR-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansTW-Regular.otf",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
            "/usr/share/fonts/opentype/ipafont-mincho/ipam.ttf",
        ],
    );

    #[cfg(target_os = "macos")]
    extend_if_exists(
        &mut candidates,
        None,
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
        ],
    );

    #[cfg(target_os = "windows")]
    {
        let windows_dir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let local_fonts_dir =
            dirs::data_local_dir().map(|dir| dir.join(r"Microsoft\Windows\Fonts"));

        extend_if_exists(
            &mut candidates,
            Some(windows_dir),
            &[
                r"Fonts\msyh.ttc",
                r"Fonts\msyh.ttf",
                r"Fonts\msyhbd.ttc",
                r"Fonts\YuGothR.ttc",
                r"Fonts\YuGothM.ttc",
                r"Fonts\meiryo.ttc",
                r"Fonts\msgothic.ttc",
                r"Fonts\malgun.ttf",
                r"Fonts\simsun.ttc",
            ],
        );
        extend_if_exists(
            &mut candidates,
            local_fonts_dir,
            &[
                "msyh.ttc",
                "msyh.ttf",
                "msyhbd.ttc",
                "YuGothR.ttc",
                "YuGothM.ttc",
                "meiryo.ttc",
                "msgothic.ttc",
                "malgun.ttf",
                "simsun.ttc",
            ],
        );
    }

    dedupe_paths(candidates)
}

fn extend_if_exists(target: &mut Vec<PathBuf>, base: Option<PathBuf>, suffixes: &[&str]) {
    for suffix in suffixes {
        let path = base
            .as_ref()
            .map(|dir| dir.join(suffix))
            .unwrap_or_else(|| PathBuf::from(suffix));
        if path_exists(&path) {
            target.push(path);
        }
    }
}

fn path_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    deduped
}

fn init_toggle_hotkey(
    hotkey_text: &str,
) -> (
    Option<GlobalHotKeyManager>,
    Option<HotKey>,
    Option<(String, bool)>,
) {
    let hotkey = match hotkey_text.parse::<HotKey>() {
        Ok(hotkey) => hotkey,
        Err(err) => {
            tracing::warn!("failed to parse toggle hotkey '{}': {}", hotkey_text, err);
            return (
                None,
                None,
                Some((format!("Invalid toggle hotkey: {}", hotkey_text), true)),
            );
        }
    };

    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(err) => {
            tracing::warn!("failed to create global hotkey manager: {}", err);
            return (
                None,
                None,
                Some((
                    "Global hotkey is unavailable on this desktop session".into(),
                    true,
                )),
            );
        }
    };

    if let Err(err) = manager.register(hotkey) {
        tracing::warn!(
            "failed to register global hotkey '{}': {}",
            hotkey_text,
            err
        );
        return (
            None,
            None,
            Some((format!("Failed to register hotkey {}", hotkey_text), true)),
        );
    }

    (Some(manager), Some(hotkey), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_ring_counts_uses_two_rings_after_eight_items() {
        assert_eq!(radial_ring_counts(0), (0, 0));
        assert_eq!(radial_ring_counts(4), (4, 0));
        assert_eq!(radial_ring_counts(8), (8, 0));
        assert_eq!(radial_ring_counts(11), (8, 3));
    }

    #[test]
    fn clamp_to_view_falls_back_to_midpoint_for_small_windows() {
        assert_eq!(clamp_to_view(50.0, 10.0, 30.0), 30.0);
        assert_eq!(clamp_to_view(50.0, 40.0, 20.0), 30.0);
    }

    #[test]
    fn radial_sector_index_starts_at_top_and_rotates_clockwise() {
        assert_eq!(radial_sector_index(egui::vec2(0.0, -50.0), 8), Some(0));
        assert_eq!(radial_sector_index(egui::vec2(50.0, 0.0), 8), Some(2));
        assert_eq!(radial_sector_index(egui::vec2(0.0, 50.0), 8), Some(4));
        assert_eq!(radial_sector_index(egui::vec2(-50.0, 0.0), 8), Some(6));
    }

    #[test]
    fn radial_hovered_entry_distinguishes_center_inner_and_outer_ring() {
        let origin = egui::pos2(100.0, 100.0);
        let counts = (8, 4);

        assert_eq!(
            radial_hovered_entry(origin, egui::pos2(100.0, 100.0), counts),
            None
        );
        assert_eq!(
            radial_hovered_entry(origin, egui::pos2(100.0, 40.0), counts),
            Some(0)
        );
        assert_eq!(
            radial_hovered_entry(origin, egui::pos2(100.0, -40.0), counts),
            Some(8)
        );
    }
}
