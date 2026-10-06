use crate::action::{
    Action, ActionExecutionControl, ActionKind, ExecResult, LowCodeClipboardFormat,
    LowCodeKeyMacroStep, LowCodePluginDraft, LowCodePluginKind, LowCodePluginStep,
    LowCodeStringProcessMethod, LowCodeWriteClipboardKind,
};
use crate::config::Config;
use crate::focus::{self, FocusTracker};
use crate::search::SearchEngine;
use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use global_hotkey::hotkey::HotKey;
#[cfg(not(target_arch = "wasm32"))]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::BTreeSet;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::mpsc::{self, Receiver};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
#[cfg(target_arch = "wasm32")]
use web_time::{Duration, Instant};

mod basic_editor;
mod editor;
mod fonts;
mod overlay;
mod panel;
mod settings;

use self::fonts::install_cjk_font_fallbacks;
use self::settings::SettingsPage;

const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(800);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const RADIAL_MAX_INNER_ITEMS: usize = 8;
const RADIAL_CENTER_RADIUS: f32 = 34.0;
const RADIAL_INNER_RADIUS: f32 = 108.0;
const RADIAL_OUTER_RADIUS: f32 = 168.0;
const RADIAL_SELECTION_PADDING: f32 = 28.0;
const RADIAL_OVERLAY_MARGIN: f32 = RADIAL_OUTER_RADIUS + 8.0;

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
    path: Vec<usize>,
    action_idx: usize,
    action: Action,
}

struct RadialMenuState {
    origin: egui::Pos2,
    pointer: egui::Pos2,
    entries: Vec<RadialMenuEntry>,
}

/// Notification that disappears after a timeout.
struct Toast {
    message: String,
    is_error: bool,
    expires: Instant,
}

struct ActionExecutionMessage {
    action_name: String,
    result: ExecResult,
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

#[derive(Clone)]
struct StepDragPayload {
    scope: String,
    from: usize,
}

#[derive(Default)]
struct StepCardAction {
    remove: bool,
}

enum PluginEditorMode {
    LowCode,
    RawJson { reason: String },
}

#[derive(Clone)]
struct ActionEditTarget {
    profile_idx: usize,
    path: Vec<usize>,
    action_idx: usize,
}

pub struct QuickerApp {
    config: Config,
    #[cfg(target_os = "linux")]
    activation: Option<crate::activation::Server>,
    #[cfg(target_os = "linux")]
    deferred_input_action: Option<(Action, focus::FocusedProcess)>,
    quitting: bool,
    search: SearchEngine,
    #[cfg(not(target_arch = "wasm32"))]
    _hotkey_manager: Option<GlobalHotKeyManager>,
    #[cfg(not(target_arch = "wasm32"))]
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

    basic_draft: Option<Action>,

    // Plugin editor state
    edit_field1: String, // raw Quicker JSON
    plugin_draft: LowCodePluginDraft,
    plugin_editor_mode: PluginEditorMode,
    plugin_new_key_macro_step_idx: usize,
    plugin_new_step_idx: usize,
    edit_target: Option<ActionEditTarget>,
    action_control: Option<ActionExecutionControl>,
    pending_action_name: Option<String>,
    action_result_rx: Option<Receiver<ActionExecutionMessage>>,
    settings_page: SettingsPage,
    settings_search: String,
}

impl QuickerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
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

        #[cfg(not(target_arch = "wasm32"))]
        let (hotkey_manager, toggle_hotkey, startup_notice) =
            init_toggle_hotkey(&config.toggle_hotkey);
        #[cfg(target_arch = "wasm32")]
        let startup_notice = Some((
            "Web preview mode is active. Links and copy actions work here; desktop integrations stay in the native build."
                .into(),
            false,
        ));

        Self {
            config,
            #[cfg(target_os = "linux")]
            activation: None,
            #[cfg(target_os = "linux")]
            deferred_input_action: None,
            quitting: false,
            search: SearchEngine::new(),
            #[cfg(not(target_arch = "wasm32"))]
            _hotkey_manager: hotkey_manager,
            #[cfg(not(target_arch = "wasm32"))]
            toggle_hotkey,
            query: String::new(),
            active_profile: 0,
            focus_tracker: FocusTracker::new(current_process_id()),
            last_focus_poll: Instant::now() - FOCUS_POLL_INTERVAL,
            needs_focus_profile_sync: true,
            action_scope: None,
            view: View::Panel,
            toast: None,
            script_output: String::new(),
            radial_menu: None,
            panel_hidden: false,
            startup_notice,
            basic_draft: Some(basic_editor::blank_action()),
            edit_field1: String::new(),
            plugin_draft: LowCodePluginDraft::default(),
            plugin_editor_mode: PluginEditorMode::LowCode,
            plugin_new_key_macro_step_idx: 0,
            plugin_new_step_idx: 0,
            edit_target: None,
            action_control: None,
            pending_action_name: None,
            action_result_rx: None,
            settings_page: SettingsPage::Launcher,
            settings_search: String::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_initial_focus(&mut self, process: Option<focus::FocusedProcess>) {
        self.focus_tracker.observe(process);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_initial_visibility(&mut self, hidden: bool) {
        self.panel_hidden = hidden;
    }

    #[cfg(target_os = "linux")]
    pub fn set_activation(&mut self, server: Option<crate::activation::Server>) {
        self.activation = server;
    }

    fn show_panel(&mut self, ctx: &egui::Context) {
        self.refresh_focused_process();
        self.panel_hidden = false;
        self.view = View::Panel;
        self.needs_focus_profile_sync = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide_panel(&mut self, ctx: &egui::Context) {
        self.panel_hidden = true;
        self.radial_menu = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    #[cfg(target_os = "linux")]
    fn poll_activation(&mut self, ctx: &egui::Context) {
        for _ in 0..16 {
            let request = match self.activation.as_ref().map(|server| server.receive()) {
                Some(Ok(Some(request))) => request,
                _ => break,
            };
            use crate::activation::Request;
            match request {
                Request::Show => self.show_panel(ctx),
                Request::Hide => self.hide_panel(ctx),
                Request::Toggle if self.panel_hidden => self.show_panel(ctx),
                Request::Toggle => self.hide_panel(ctx),
                Request::Quit => {
                    self.quitting = true;
                    if let Some(control) = &self.action_control {
                        control.cancel();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn apply_settings(&mut self, ctx: &egui::Context) {
        if let Err(err) = self.config.validate() {
            self.show_toast(err, true);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let new_hotkey = self
            .config
            .toggle_hotkey
            .parse::<HotKey>()
            .expect("validated hotkey");
        #[cfg(not(target_arch = "wasm32"))]
        let rebind = self.toggle_hotkey != Some(new_hotkey);
        #[cfg(not(target_arch = "wasm32"))]
        if rebind {
            if let Some(manager) = &self._hotkey_manager {
                if let Err(err) = manager.register(new_hotkey) {
                    self.show_toast(format!("Cannot register shortcut: {err}"), true);
                    return;
                }
            }
        }
        if !self.save_config() {
            #[cfg(not(target_arch = "wasm32"))]
            if rebind {
                if let Some(manager) = &self._hotkey_manager {
                    let _ = manager.unregister(new_hotkey);
                }
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if rebind {
            if let Some(manager) = &self._hotkey_manager {
                if let Some(previous) = self.toggle_hotkey {
                    let _ = manager.unregister(previous);
                }
                self.toggle_hotkey = Some(new_hotkey);
            } else {
                let (manager, hotkey, notice) = init_toggle_hotkey(&self.config.toggle_hotkey);
                self._hotkey_manager = manager;
                self.toggle_hotkey = hotkey;
                self.startup_notice = notice;
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            self.config.panel_width,
            self.config.panel_height,
        )));
        self.needs_focus_profile_sync = true;
        self.show_toast("Settings applied and saved".into(), false);
    }

    fn save_config(&mut self) -> bool {
        match self.config.save() {
            Ok(()) => true,
            Err(err) => {
                self.show_toast(err, true);
                false
            }
        }
    }

    fn show_toast(&mut self, msg: String, is_error: bool) {
        self.toast = Some(Toast {
            message: msg,
            is_error,
            expires: Instant::now() + Duration::from_secs(3),
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

    fn execute_action(&mut self, ctx: &egui::Context, action: &Action) {
        if let Some(name) = &self.pending_action_name {
            self.show_toast(format!("Action still running: {name}"), true);
            return;
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.execute_action_preview(ctx, action);
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(target_os = "linux")]
            let input_target = if action.needs_input_target() {
                self.refresh_focused_process();
                let Some(target) = self.focus_tracker.current_external().cloned() else {
                    self.show_toast(
                        "Focus a target application before running keyboard automation".into(),
                        true,
                    );
                    return;
                };
                if !self.panel_hidden {
                    // Start on the next frame, after eframe has applied the hide
                    // command. The worker then verifies the recipient's focus.
                    self.hide_panel(ctx);
                    self.deferred_input_action = Some((action.clone(), target));
                    ctx.request_repaint();
                    return;
                }
                Some(target)
            } else {
                None
            };
            #[cfg(not(target_os = "linux"))]
            let input_target = None;
            self.start_action(ctx, action, input_target);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_action(
        &mut self,
        ctx: &egui::Context,
        action: &Action,
        _input_target: Option<focus::FocusedProcess>,
    ) {
        let action_name = action.name.clone();
        let action_clone = action.clone();
        let control = ActionExecutionControl::new();
        let (tx, rx) = mpsc::channel();
        let repaint_ctx = ctx.clone();
        self.action_control = Some(control.clone());
        self.pending_action_name = Some(action_name.clone());
        self.action_result_rx = Some(rx);

        thread::spawn(move || {
            #[cfg(target_os = "linux")]
            let focus_result = _input_target
                .as_ref()
                .map_or(Ok(()), |target| crate::x11::restore_focus(target, &control));
            #[cfg(not(target_os = "linux"))]
            let focus_result: Result<(), String> = Ok(());
            let result = match focus_result {
                Ok(()) => action_clone.execute_with_control(Some(&control)),
                Err(err) => ExecResult::Err(err),
            };
            let _ = tx.send(ActionExecutionMessage {
                action_name,
                result,
            });
            repaint_ctx.request_repaint();
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn execute_action_preview(&mut self, ctx: &egui::Context, action: &Action) {
        let action_name = action.name.clone();
        let result = match &action.kind {
            ActionKind::OpenUrl { url } => {
                ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
                ExecResult::OkWithMessage(format!("Opened {url}"))
            }
            ActionKind::CopyText { text } => {
                ctx.copy_text(text.clone());
                ExecResult::OkWithMessage("Copied to clipboard".into())
            }
            ActionKind::Group { .. } => ExecResult::Ok,
            ActionKind::OpenFile { .. }
            | ActionKind::OpenFolder { .. }
            | ActionKind::RunProgram { .. }
            | ActionKind::RunShell { .. }
            | ActionKind::SearchClipboardText { .. }
            | ActionKind::OpenClipboardText { .. }
            | ActionKind::RunClipboardText { .. }
            | ActionKind::PluginPipeline { .. } => ExecResult::Err(
                "This action depends on native OS integrations and is disabled in the web preview."
                    .into(),
            ),
        };
        self.handle_exec_result(&action_name, result);
    }

    fn request_cancel_running_action(&mut self, ctx: &egui::Context) {
        let Some(control) = &self.action_control else {
            return;
        };
        if control.is_cancelled() {
            return;
        }
        control.cancel();
        self.show_toast("Cancelling action...".into(), false);
        ctx.request_repaint();
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

        self.refresh_focused_process();
    }

    fn refresh_focused_process(&mut self) {
        self.last_focus_poll = Instant::now();
        // A workflow dialog is part of this app. Keep the external input target.
        if self
            .action_control
            .as_ref()
            .is_some_and(ActionExecutionControl::dialog_active)
        {
            return;
        }
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
        self.basic_draft = Some(basic_editor::blank_action());
        self.edit_field1.clear();
        self.plugin_draft = LowCodePluginDraft::default();
        self.plugin_editor_mode = PluginEditorMode::LowCode;
        self.plugin_new_key_macro_step_idx = 0;
        self.plugin_new_step_idx = 0;
        self.edit_target = None;
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
        path: Vec<usize>,
        actions: &[Action],
    ) -> Vec<ActionListEntry> {
        actions
            .iter()
            .cloned()
            .enumerate()
            .map(|(action_idx, action)| ActionListEntry {
                profile_idx,
                section,
                path: path.clone(),
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
                scope.path.clone(),
                self.actions_for_scope(scope),
            );
        }

        let mut entries = self.action_entries(
            self.global_profile_idx(),
            ActionSection::GlobalTools,
            Vec::new(),
            self.profile_actions(self.global_profile_idx()),
        );

        if let Some(profile_idx) = self.active_window_profile_index() {
            entries.extend(self.action_entries(
                profile_idx,
                ActionSection::ActiveWindowTools,
                Vec::new(),
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

    fn actions_mut_for_target(&mut self, target: &ActionEditTarget) -> Option<&mut Vec<Action>> {
        let profile = self.config.profiles.get_mut(target.profile_idx)?;
        Self::actions_at_path_mut(&mut profile.actions, &target.path)
    }

    fn replace_action(&mut self, target: &ActionEditTarget, mut action: Action) -> bool {
        let plugin_editor = self.basic_draft.is_none();
        let Some(actions) = self.actions_mut_for_target(target) else {
            return false;
        };
        let Some(slot) = actions.get_mut(target.action_idx) else {
            return false;
        };
        if plugin_editor {
            action.hotkey = slot.hotkey.clone();
            action.tags = slot.tags.clone();
        }
        *slot = action;
        true
    }

    fn delete_action(&mut self, target: &ActionEditTarget) -> bool {
        let Some(actions) = self.actions_mut_for_target(target) else {
            return false;
        };
        if target.action_idx >= actions.len() {
            return false;
        }
        actions.remove(target.action_idx);
        true
    }

    fn open_plugin_editor_for_entry(&mut self, entry: &ActionListEntry) {
        self.reset_editor();
        self.edit_target = Some(ActionEditTarget {
            profile_idx: entry.profile_idx,
            path: entry.path.clone(),
            action_idx: entry.action_idx,
        });
        self.view = View::ActionEditor;
        let ActionKind::PluginPipeline { plugin } = &entry.action.kind else {
            self.basic_draft = Some(entry.action.clone());
            return;
        };
        self.basic_draft = None;
        self.edit_field1 = entry
            .action
            .to_quicker_plugin_json()
            .unwrap_or_else(|_| plugin.quicker_json.clone());
        match LowCodePluginDraft::from_quicker_plugin_json(&plugin.quicker_json) {
            Ok(draft) => {
                self.plugin_draft = draft;
                self.plugin_editor_mode = PluginEditorMode::LowCode;
            }
            Err(err) => {
                self.plugin_editor_mode = PluginEditorMode::RawJson {
                    reason: err.clone(),
                };
                self.show_toast(
                    "This plugin uses unsupported builder features. Opened in raw JSON mode."
                        .into(),
                    false,
                );
            }
        }
        self.edit_target = Some(ActionEditTarget {
            profile_idx: entry.profile_idx,
            path: entry.path.clone(),
            action_idx: entry.action_idx,
        });
        self.view = View::ActionEditor;
    }

    fn persist_edited_or_new_action(&mut self, action: Action) {
        if action.name.trim().is_empty() {
            self.show_toast("Action name is required".into(), true);
            return;
        }
        let previous_config = self.config.clone();
        let message = if let Some(target) = self.edit_target.clone() {
            if !self.replace_action(&target, action) {
                self.show_toast("Failed to update action.".into(), true);
                return;
            }
            "Action updated!"
        } else {
            if let Some(actions) = self.current_actions_mut() {
                actions.push(action);
            }
            "Action added!"
        };

        if !self.save_config() {
            self.config = previous_config;
            return;
        }
        self.edit_target = None;
        self.show_toast(message.into(), false);
        self.view = View::Panel;
        self.needs_focus_profile_sync = true;
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
}

impl eframe::App for QuickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // eframe maps its root window after the first rendered frame even when
        // the viewport builder requested hidden startup. Reassert on updates.
        #[cfg(not(target_arch = "wasm32"))]
        if self.panel_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        #[cfg(target_os = "linux")]
        self.poll_activation(ctx);
        #[cfg(target_os = "linux")]
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_panel(ctx);
        }
        self.show_startup_notice_once();
        self.handle_global_hotkey(ctx);
        self.poll_action_result(ctx);
        #[cfg(target_os = "linux")]
        if let Some((action, target)) = self.deferred_input_action.take() {
            self.start_action(ctx, &action, Some(target));
        }

        // Handle Escape key
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.pending_action_name.is_some() {
                self.request_cancel_running_action(ctx);
            } else if self.radial_menu.is_some() {
                self.cancel_radial_menu(ctx);
                ctx.request_repaint();
            } else if self.view != View::Panel {
                self.view = View::Panel;
                self.needs_focus_profile_sync = true;
            } else if self.action_scope.is_some() {
                self.leave_group();
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                self.hide_panel(ctx);
            }
        }

        self.poll_focused_process();
        if self.view == View::Panel && self.needs_focus_profile_sync {
            self.sync_profile_to_focus();
            self.needs_focus_profile_sync = false;
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

        #[cfg(not(target_arch = "wasm32"))]
        crate::text_windows::render(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        crate::wait_windows::render(ctx);
        self.render_radial_menu(ctx);
        self.render_toast(ctx);
        self.render_running_action(ctx);
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
            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(32, 87, 184))
        } else {
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(210))
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

#[cfg(not(target_arch = "wasm32"))]
fn init_toggle_hotkey(
    hotkey_text: &str,
) -> (
    Option<GlobalHotKeyManager>,
    Option<HotKey>,
    Option<(String, bool)>,
) {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return (
            None,
            None,
            Some((
                "Wayland: bind quicker-rs --toggle in your desktop shortcut settings".into(),
                false,
            )),
        );
    }
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

#[cfg(not(target_arch = "wasm32"))]
fn current_process_id() -> u32 {
    std::process::id()
}

#[cfg(target_arch = "wasm32")]
fn current_process_id() -> u32 {
    0
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
