//! X11 desktop integration, independent of the launcher's own window.
use crate::action::ActionExecutionControl;
use crate::focus::FocusedProcess;
use std::time::{Duration, Instant};
use xcb::{x, Xid, XidNew};
mod keyboard;
mod selection;
mod windows;
pub(crate) use keyboard::Keyboard;
pub(crate) use selection::read_file_selection;
pub(crate) use windows::{WindowMatch, WindowQuery};

xcb::atoms_struct! {
    struct Atoms {
        pid => b"_NET_WM_PID" only_if_exists = false,
        active => b"_NET_ACTIVE_WINDOW" only_if_exists = false,
        wm_check => b"_NET_SUPPORTING_WM_CHECK" only_if_exists = false,
        clients => b"_NET_CLIENT_LIST_STACKING" only_if_exists = false,
        client_list => b"_NET_CLIENT_LIST" only_if_exists = false,
        title => b"_NET_WM_NAME" only_if_exists = false,
        utf8 => b"UTF8_STRING" only_if_exists = false,
    }
}

struct Desktop {
    conn: xcb::Connection,
    root: x::Window,
    atoms: Atoms,
}

impl Desktop {
    fn connect() -> Result<Self, String> {
        let (conn, screen) = xcb::Connection::connect(None).map_err(|e| e.to_string())?;
        let root = conn
            .get_setup()
            .roots()
            .nth(screen as usize)
            .ok_or("X11 screen is unavailable")?
            .root();
        let atoms = Atoms::intern_all(&conn).map_err(|e| e.to_string())?;
        Ok(Self { conn, root, atoms })
    }

    fn property(
        &self,
        window: x::Window,
        property: x::Atom,
        kind: x::Atom,
    ) -> Result<x::GetPropertyReply, String> {
        self.conn
            .wait_for_reply(self.conn.send_request(&x::GetProperty {
                delete: false,
                window,
                property,
                r#type: kind,
                long_offset: 0,
                long_length: 1024,
            }))
            .map_err(|e| e.to_string())
    }

    fn pid(&self, window: x::Window) -> u32 {
        self.property(window, self.atoms.pid, x::ATOM_CARDINAL)
            .ok()
            .filter(|reply| reply.format() == 32)
            .and_then(|reply| reply.value::<u32>().first().copied())
            .unwrap_or(0)
    }

    fn focus(&self) -> Result<x::Window, String> {
        self.conn
            .wait_for_reply(self.conn.send_request(&x::GetInputFocus {}))
            .map(|reply| reply.focus())
            .map_err(|e| e.to_string())
    }

    fn parent(&self, window: x::Window) -> Option<x::Window> {
        self.conn
            .wait_for_reply(self.conn.send_request(&x::QueryTree { window }))
            .ok()
            .map(|r| r.parent())
    }

    fn belongs_to(&self, mut window: x::Window, target: x::Window) -> bool {
        for _ in 0..32 {
            if window == target {
                return true;
            }
            if window.resource_id() <= 1 || window == self.root {
                break;
            }
            let Some(parent) = self.parent(window) else {
                break;
            };
            window = parent;
        }
        false
    }
}

pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland")
}

pub fn focused_process() -> Option<FocusedProcess> {
    let desktop = Desktop::connect().ok()?;
    let mut window = desktop.focus().ok()?;
    let mut client = None;
    for _ in 0..32 {
        if window.resource_id() <= 1 || window == desktop.root {
            break;
        }
        if let Ok(class) = desktop.property(window, x::ATOM_WM_CLASS, x::ATOM_STRING) {
            if class.format() == 8 && !class.value::<u8>().is_empty() {
                let parts: Vec<_> = class
                    .value::<u8>()
                    .split(|byte| *byte == 0)
                    .filter(|part| !part.is_empty())
                    .collect();
                let app_name = String::from_utf8_lossy(parts.last()?).to_string();
                client = Some((window, app_name));
            }
        }
        window = desktop.parent(window)?;
    }
    let (window, app_name) = client?;
    let process_id = desktop.pid(window);
    let process_path = std::fs::read_link(format!("/proc/{process_id}/exe"))
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    Some(FocusedProcess {
        app_name,
        process_id,
        process_path,
        window_id: window.resource_id().to_string(),
    })
}

/// Read PRIMARY only when its owner belongs to the focused application window.
/// A previous selection in another application is not the current selection.
pub fn focused_selection_text() -> Result<Option<String>, String> {
    use arboard::{GetExtLinux, LinuxClipboardKind};
    if is_wayland() {
        return Err("Focused selection access requires an X11 session".into());
    }
    let Some(target) = focused_process() else {
        return Ok(None);
    };
    let window = x::Window::new(
        target
            .window_id
            .parse()
            .map_err(|_| "Invalid focused window")?,
    );
    let desktop = Desktop::connect()?;
    let owner = || {
        desktop
            .conn
            .wait_for_reply(desktop.conn.send_request(&x::GetSelectionOwner {
                selection: x::ATOM_PRIMARY,
            }))
            .map(|r| r.owner())
            .map_err(|e| e.to_string())
    };
    let before = owner()?;
    if !desktop.belongs_to(before, window) {
        return Ok(None);
    }
    let text = arboard::Clipboard::new()
        .map_err(|e| e.to_string())?
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .map_err(|e| e.to_string())?;
    if owner()? != before || !desktop.belongs_to(desktop.focus()?, window) {
        return Err("The focused selection changed during the read".into());
    }
    Ok(Some(text))
}

pub fn restore_focus(
    target: &FocusedProcess,
    control: &ActionExecutionControl,
) -> Result<(), String> {
    if is_wayland() {
        return Err("Native Wayland input is not configured for this session".into());
    }
    let id: u32 = target
        .window_id
        .parse()
        .map_err(|_| "Invalid target X11 window")?;
    if id <= 1 {
        return Err("No target application window was captured".into());
    }
    let desktop = Desktop::connect()?;
    let window = x::Window::new(id);
    let actual_pid = desktop.pid(window);
    if target.process_id != 0 && actual_pid != target.process_id {
        return Err("The target application window has closed or changed".into());
    }
    // Honor the window manager's activation protocol (workspace switching,
    // unminimizing, focus policy) instead of forcing input focus behind its back.
    let managed = desktop
        .property(desktop.root, desktop.atoms.wm_check, x::ATOM_WINDOW)
        .is_ok_and(|reply| reply.format() == 32 && !reply.value::<u32>().is_empty());
    if managed {
        let event = x::ClientMessageEvent::new(
            window,
            desktop.atoms.active,
            x::ClientMessageData::Data32([2, x::CURRENT_TIME, 0, 0, 0]),
        );
        desktop
            .conn
            .send_and_check_request(&x::SendEvent {
                propagate: false,
                destination: x::SendEventDest::Window(desktop.root),
                event_mask: x::EventMask::SUBSTRUCTURE_REDIRECT | x::EventMask::SUBSTRUCTURE_NOTIFY,
                event: &event,
            })
            .map_err(|e| e.to_string())?;
    } else {
        // Bare X servers (including the test display) have no window manager.
        desktop
            .conn
            .send_and_check_request(&x::SetInputFocus {
                revert_to: x::InputFocus::Parent,
                focus: window,
                time: x::CURRENT_TIME,
            })
            .map_err(|e| e.to_string())?;
    }
    let deadline = Instant::now() + Duration::from_millis(900);
    while Instant::now() < deadline {
        if control.is_cancelled() {
            return Err("Action cancelled".into());
        }
        if desktop.belongs_to(desktop.focus()?, window) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    Err("The desktop did not focus the target application; no keys were sent".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn focused_selection_rejects_primary_owned_by_another_window() {
        let desktop = Desktop::connect().unwrap();
        let focused = desktop.conn.generate_id();
        let foreign = desktop.conn.generate_id();
        for window in [focused, foreign] {
            desktop
                .conn
                .send_and_check_request(&x::CreateWindow {
                    depth: 0,
                    wid: window,
                    parent: desktop.root,
                    x: 0,
                    y: 0,
                    width: 30,
                    height: 30,
                    border_width: 0,
                    class: x::WindowClass::InputOutput,
                    visual: x::COPY_FROM_PARENT,
                    value_list: &[],
                })
                .unwrap();
            desktop
                .conn
                .send_and_check_request(&x::ChangeProperty {
                    mode: x::PropMode::Replace,
                    window,
                    property: x::ATOM_WM_CLASS,
                    r#type: x::ATOM_STRING,
                    data: b"selection-test\0SelectionTest\0",
                })
                .unwrap();
            desktop
                .conn
                .send_and_check_request(&x::MapWindow { window })
                .unwrap();
        }
        desktop
            .conn
            .send_and_check_request(&x::SetInputFocus {
                revert_to: x::InputFocus::Parent,
                focus: focused,
                time: x::CURRENT_TIME,
            })
            .unwrap();
        desktop
            .conn
            .send_and_check_request(&x::SetSelectionOwner {
                owner: foreign,
                selection: x::ATOM_PRIMARY,
                time: x::CURRENT_TIME,
            })
            .unwrap();
        assert_eq!(
            focused_process().unwrap().window_id,
            focused.resource_id().to_string()
        );
        assert_eq!(focused_selection_text().unwrap(), None);
    }
}
