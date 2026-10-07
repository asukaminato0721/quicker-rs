//! One waiting window per root action. Subprograms share the same session.
use crate::action::ActionExecutionControl;
use egui::{Context, Pos2, ViewportId};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

mod native;
pub(crate) use native::render;

#[derive(Clone, Debug)]
pub(crate) struct Content {
    pub title: String,
    pub prompt: String,
    pub button: String,
    pub progress: Option<f32>,
    pub progress_text: String,
    pub operations: Vec<(String, String)>,
}

pub(crate) struct Options {
    pub location: String,
    pub activation: String,
    pub font_size: f32,
    pub auto_close: f64,
    pub stop_on_close: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub closed: bool,
    pub shown: bool,
    pub operation: String,
    pub error: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            closed: true,
            shown: false,
            operation: String::new(),
            error: None,
        }
    }
}

struct Window {
    id: u64,
    content: Content,
    options: Options,
    result: Snapshot,
    control: ActionExecutionControl,
    created: Instant,
    shown_at: Option<Instant>,
    position: Option<Pos2>,
    configured: bool,
    native_window: Option<u32>,
    scale: f32,
}
type Handle = Arc<Mutex<Window>>;

#[derive(Default)]
pub(crate) struct Session {
    window: Mutex<Option<Handle>>,
}

struct Host {
    context: Context,
    windows: Mutex<Vec<Weak<Mutex<Window>>>>,
}
static HOST: OnceLock<Host> = OnceLock::new();
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn repaint(id: u64) {
    if let Some(host) = HOST.get() {
        host.context.request_repaint_of(ViewportId::ROOT);
        host.context.request_repaint_of(viewport(id));
    }
}
fn viewport(id: u64) -> ViewportId {
    ViewportId::from_hash_of(("workflow-wait", id))
}

impl Session {
    pub(crate) fn is_closed(&self) -> bool {
        self.window
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|w| w.lock().unwrap().result.closed)
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.window
            .lock()
            .unwrap()
            .as_ref()
            .map(|w| w.lock().unwrap().result.clone())
            .unwrap_or_default()
    }

    pub(crate) fn update(&self, content: Content) {
        if let Some(handle) = self.window.lock().unwrap().as_ref() {
            let mut window = handle.lock().unwrap();
            if !window.result.closed {
                window.content = content;
                repaint(window.id);
            }
        }
    }

    pub(crate) fn show(
        &self,
        content: Content,
        options: Options,
        control: ActionExecutionControl,
    ) -> Result<(), String> {
        let mut slot = self.window.lock().unwrap();
        if let Some(handle) = slot.as_ref() {
            let mut window = handle.lock().unwrap();
            if !window.result.closed {
                // Showing an existing window changes its content, not its lifecycle options.
                window.content = content;
                window.result.operation.clear();
                repaint(window.id);
                return Ok(());
            }
        }
        let host = HOST
            .get()
            .ok_or("Wait windows require the running native application")?;
        #[cfg(target_os = "linux")]
        if crate::x11::is_wayland() {
            return Err("Wait-window focus and placement require an X11 session".into());
        }
        #[cfg(not(target_os = "linux"))]
        return Err("Wait windows currently require Linux X11".into());
        let position = if options.location == "LastPosition" {
            slot.as_ref().and_then(|w| w.lock().unwrap().position)
        } else {
            None
        };
        let mut windows = host.windows.lock().unwrap();
        windows.retain(|w| {
            w.upgrade()
                .is_some_and(|w| !w.lock().unwrap().result.closed)
        });
        if windows.len() >= 32 {
            return Err("At most 32 wait windows can be open".into());
        }
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let handle = Arc::new(Mutex::new(Window {
            id,
            content,
            options,
            control,
            position,
            result: Snapshot {
                closed: false,
                ..Default::default()
            },
            created: Instant::now(),
            shown_at: None,
            configured: false,
            native_window: None,
            scale: host.context.pixels_per_point(),
        }));
        windows.push(Arc::downgrade(&handle));
        *slot = Some(handle);
        repaint(id);
        Ok(())
    }

    pub(crate) fn close(&self) {
        if let Some(handle) = self.window.lock().unwrap().as_ref() {
            let mut window = handle.lock().unwrap();
            window.capture_position();
            window.result.closed = true;
            repaint(window.id);
        }
    }

    pub(crate) fn wait(
        &self,
        until_shown: bool,
        control: Option<&ActionExecutionControl>,
    ) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if control.is_some_and(ActionExecutionControl::is_cancelled) {
                self.close();
                return Err("Action cancelled".into());
            }
            let result = self.snapshot();
            if let Some(error) = result.error {
                return Err(error);
            }
            if result.closed || (until_shown && result.shown) {
                return Ok(());
            }
            if !result.shown && start.elapsed() > Duration::from_secs(10) {
                self.close();
                return Err("Wait window did not open within 10 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

impl Window {
    fn capture_position(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(window) = self.native_window {
            if let Ok(position) = crate::x11::wait_window::position(window, self.scale) {
                self.position = Some(position);
            }
        }
    }

    // Programmatic close and return buttons bypass stopActionIfClose. In this MSI,
    // automatic closure uses the same stop behavior as the title-bar close button.
    fn dismiss(&mut self, operation: Option<String>) {
        self.capture_position();
        if operation.is_none() && self.options.stop_on_close {
            self.control.cancel();
        }
        self.result.operation = operation.unwrap_or_default();
        self.result.closed = true;
        repaint(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Options {
        Options {
            location: "CenterScreen".into(),
            activation: "NotActivatable".into(),
            font_size: 12.0,
            auto_close: 0.0,
            stop_on_close: true,
        }
    }
    fn content(title: &str) -> Content {
        Content {
            title: title.into(),
            prompt: "prompt".into(),
            button: "Done".into(),
            progress: None,
            progress_text: String::new(),
            operations: Vec::new(),
        }
    }
    fn session() -> (Session, Handle, ActionExecutionControl) {
        let control = ActionExecutionControl::new();
        let window = Arc::new(Mutex::new(Window {
            id: 0,
            content: content("first"),
            options: options(),
            result: Snapshot {
                closed: false,
                shown: true,
                ..Default::default()
            },
            control: control.clone(),
            created: Instant::now(),
            shown_at: Some(Instant::now()),
            position: Some(egui::pos2(10.0, 20.0)),
            configured: true,
            native_window: None,
            scale: 1.0,
        }));
        (
            Session {
                window: Mutex::new(Some(window.clone())),
            },
            window,
            control,
        )
    }

    #[test]
    fn wait_window_updates_preserve_identity_and_lifecycle_options() {
        let (session, window, control) = session();
        let mut changed = options();
        changed.auto_close = 1.0;
        changed.stop_on_close = false;
        changed.activation = "AutoActivate".into();
        session
            .show(content("shown again"), changed, control)
            .unwrap();
        let existing = session.window.lock().unwrap().clone().unwrap();
        assert!(Arc::ptr_eq(&existing, &window));
        session.update(content("updated"));
        {
            let window = window.lock().unwrap();
            assert_eq!(window.content.title, "updated");
            assert_eq!(window.options.auto_close, 0.0);
            assert!(window.options.stop_on_close);
            assert_eq!(window.options.activation, "NotActivatable");
        }
        session.close();
        session.update(content("ignored"));
        assert_eq!(window.lock().unwrap().content.title, "updated");
    }

    #[test]
    fn wait_window_close_sources_preserve_cancellation_and_return_values() {
        for button in [Some("chosen".to_string()), Some(String::new()), None] {
            let (session, window, control) = session();
            window.lock().unwrap().dismiss(button.clone());
            assert!(session.is_closed());
            assert_eq!(control.is_cancelled(), button.is_none());
            assert_eq!(session.snapshot().operation, button.unwrap_or_default());
        }
        let (session, window, control) = session();
        window.lock().unwrap().options.stop_on_close = false;
        window.lock().unwrap().dismiss(None);
        assert!(session.is_closed());
        assert!(!control.is_cancelled());
    }

    #[test]
    fn wait_window_shared_session_closes_only_after_last_owner() {
        let (session, window, control) = session();
        let parent = Arc::new(session);
        let child = parent.clone();
        drop(child);
        assert!(!parent.is_closed());
        drop(parent);
        assert!(window.lock().unwrap().result.closed);
        assert!(!control.is_cancelled());
    }

    #[test]
    fn wait_window_action_cancellation_closes_a_blocking_wait() {
        let (session, _window, control) = session();
        control.cancel();
        assert_eq!(
            session.wait(false, Some(&control)),
            Err("Action cancelled".into())
        );
        assert!(session.is_closed());
    }
}
