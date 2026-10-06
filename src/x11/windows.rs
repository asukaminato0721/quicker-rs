use super::*;
use crate::focus::normalize_process_name;
use fancy_regex::Regex;

pub(crate) struct WindowQuery {
    process: String,
    class: Option<Regex>,
    title: Option<Regex>,
}

pub(crate) struct WindowMatch {
    pub process: FocusedProcess,
    pub title: String,
}

impl WindowQuery {
    pub fn new(process: &str, class: &str, title: &str) -> Result<Self, String> {
        if process.trim().is_empty() {
            return Err("The process name or PID is required".into());
        }
        let regex = |text: &str| {
            if text.is_empty() {
                Ok(None)
            } else {
                Regex::new(text)
                    .map(Some)
                    .map_err(|e| format!("Invalid window filter: {e}"))
            }
        };
        Ok(Self {
            process: process.trim().into(),
            class: regex(class)?,
            title: regex(title)?,
        })
    }

    fn matches_process(&self, pid: u32, path: &str, names: &[&str]) -> bool {
        if let Ok(expected) = self.process.parse::<u32>() {
            return expected != 0 && pid == expected;
        }
        if self.process.contains('/') {
            return path == self.process
                || std::fs::canonicalize(&self.process)
                    .is_ok_and(|expected| expected == std::path::Path::new(path));
        }
        let expected = normalize_process_name(&self.process);
        names
            .iter()
            .copied()
            .chain(
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str()),
            )
            .any(|name| normalize_process_name(name) == expected)
    }

    pub fn process_running(&self) -> Result<bool, String> {
        use std::os::unix::fs::MetadataExt;
        let entries = std::fs::read_dir("/proc").map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if entry
                .metadata()
                .is_ok_and(|m| m.uid() != unsafe { libc::geteuid() })
            {
                continue;
            }
            let path = std::fs::read_link(entry.path().join("exe")).unwrap_or_default();
            let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            if self.matches_process(pid, &path.to_string_lossy(), &[name.trim()]) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn find(&self) -> Result<Option<WindowMatch>, String> {
        if is_wayland() {
            return Err("Process window activation requires an X11 session".into());
        }
        let desktop = Desktop::connect()?;
        let mut clients = Vec::new();
        for atom in [desktop.atoms.clients, desktop.atoms.client_list] {
            if let Ok(reply) = desktop.property(desktop.root, atom, x::ATOM_WINDOW) {
                if reply.format() == 32 {
                    clients = reply.value::<x::Window>().to_vec()
                }
            }
            if !clients.is_empty() {
                break;
            }
        }
        let managed = !clients.is_empty();
        if !managed {
            clients = desktop
                .conn
                .wait_for_reply(desktop.conn.send_request(&x::QueryTree {
                    window: desktop.root,
                }))
                .map_err(|e| e.to_string())?
                .children()
                .to_vec();
        }
        let focus = desktop.focus()?;
        let mut first = None;
        for window in clients.into_iter().rev() {
            let Ok(attributes) = desktop.conn.wait_for_reply(
                desktop
                    .conn
                    .send_request(&x::GetWindowAttributes { window }),
            ) else {
                continue;
            };
            if attributes.class() == x::WindowClass::InputOnly
                || (!managed && attributes.map_state() != x::MapState::Viewable)
            {
                continue;
            }
            let pid = desktop.pid(window);
            if pid == std::process::id() {
                continue;
            }
            let path = std::fs::read_link(format!("/proc/{pid}/exe"))
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
            let text = |property, kind| {
                desktop
                    .property(window, property, kind)
                    .ok()
                    .filter(|r| r.format() == 8)
                    .map(|r| {
                        String::from_utf8_lossy(r.value::<u8>())
                            .trim_end_matches('\0')
                            .to_string()
                    })
                    .unwrap_or_default()
            };
            let class = text(x::ATOM_WM_CLASS, x::ATOM_STRING);
            let classes: Vec<_> = class.split('\0').filter(|s| !s.is_empty()).collect();
            let mut names = classes.clone();
            names.push(comm.trim());
            if !self.matches_process(pid, &path, &names) {
                continue;
            }
            let mut title = text(desktop.atoms.title, desktop.atoms.utf8);
            if title.is_empty() {
                title = text(x::ATOM_WM_NAME, x::ATOM_ANY)
            }
            let matches = |regex: &Option<Regex>, value: &str| -> Result<bool, String> {
                regex.as_ref().map_or(Ok(true), |r| {
                    r.is_match(value)
                        .map_err(|e| format!("Window filter failed: {e}"))
                })
            };
            if !matches(&self.title, &title)? {
                continue;
            }
            let mut class_match = self.class.is_none();
            for name in &classes {
                class_match |= matches(&self.class, name)?
            }
            if !class_match {
                continue;
            }
            let candidate = WindowMatch {
                process: FocusedProcess {
                    app_name: classes.last().copied().unwrap_or(comm.trim()).into(),
                    process_id: pid,
                    process_path: path,
                    window_id: window.resource_id().to_string(),
                },
                title,
            };
            if desktop.belongs_to(focus, window) {
                return Ok(Some(candidate));
            }
            if first.is_none() {
                first = Some(candidate)
            }
        }
        Ok(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_query_matches_pid_executable_or_window_class() {
        let query = WindowQuery::new("123", "", "").unwrap();
        assert!(query.matches_process(123, "/bin/other", &[]));
        assert!(!query.matches_process(12, "/bin/123", &["123"]));
        let query = WindowQuery::new("Obsidian.exe", "", "").unwrap();
        assert!(query.matches_process(123, "/tmp/app/electron", &["obsidian"]));
        assert!(query.matches_process(123, "/usr/bin/obsidian", &[]));
        assert!(!query.matches_process(123, "/usr/bin/obsidian-other", &[]));
        assert!(WindowQuery::new("", "", "").is_err());
        assert!(WindowQuery::new("app", "[", "").is_err());
    }
}

#[cfg(test)]
mod x11_tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn window_query_filters_before_focus_and_returns_window_metadata() {
        let desktop = Desktop::connect().unwrap();
        let mut windows = Vec::new();
        for title in ["Alpha", "Beta"] {
            let window = desktop.conn.generate_id();
            desktop
                .conn
                .send_and_check_request(&x::CreateWindow {
                    depth: 0,
                    wid: window,
                    parent: desktop.root,
                    x: 0,
                    y: 0,
                    width: 50,
                    height: 50,
                    border_width: 0,
                    class: x::WindowClass::InputOutput,
                    visual: x::COPY_FROM_PARENT,
                    value_list: &[],
                })
                .unwrap();
            for (property, kind, bytes) in [
                (
                    x::ATOM_WM_CLASS,
                    x::ATOM_STRING,
                    b"query\0QueryTarget\0".as_slice(),
                ),
                (desktop.atoms.title, desktop.atoms.utf8, title.as_bytes()),
            ] {
                desktop
                    .conn
                    .send_and_check_request(&x::ChangeProperty {
                        mode: x::PropMode::Replace,
                        window,
                        property,
                        r#type: kind,
                        data: bytes,
                    })
                    .unwrap();
            }
            desktop
                .conn
                .send_and_check_request(&x::ChangeProperty {
                    mode: x::PropMode::Replace,
                    window,
                    property: desktop.atoms.pid,
                    r#type: x::ATOM_CARDINAL,
                    data: &[42_u32],
                })
                .unwrap();
            desktop
                .conn
                .send_and_check_request(&x::MapWindow { window })
                .unwrap();
            windows.push(window);
        }
        desktop
            .conn
            .send_and_check_request(&x::SetInputFocus {
                revert_to: x::InputFocus::Parent,
                focus: windows[0],
                time: x::CURRENT_TIME,
            })
            .unwrap();
        let selected = WindowQuery::new("querytarget.exe", "^QueryTarget$", "^Be.*$")
            .unwrap()
            .find()
            .unwrap()
            .unwrap();
        assert_eq!(selected.title, "Beta");
        assert_eq!(selected.process.process_id, 42);
        assert_eq!(
            selected.process.window_id,
            windows[1].resource_id().to_string()
        );
        restore_focus(&selected.process, &ActionExecutionControl::new()).unwrap();
        assert_eq!(desktop.focus().unwrap(), windows[1]);
        let by_pid = WindowQuery::new("42", "", "Beta")
            .unwrap()
            .find()
            .unwrap()
            .unwrap();
        assert_eq!(by_pid.process.window_id, selected.process.window_id);
        assert!(WindowQuery::new("42", "OtherClass", "Beta")
            .unwrap()
            .find()
            .unwrap()
            .is_none());
        assert!(WindowQuery::new("99", "", "Beta")
            .unwrap()
            .find()
            .unwrap()
            .is_none());
    }
}
