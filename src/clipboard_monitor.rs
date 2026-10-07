//! Track X11 clipboard events without reading or storing clipboard contents.
use std::sync::Mutex;
use xcb::{x, xfixes, Xid};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Snapshot {
    pub sequence: u64,
    pub age_ms: Option<u32>,
}

struct Monitor {
    connection: xcb::Connection,
    window: x::Window,
    clock_property: x::Atom,
    sequence: u64,
    last_change: Option<u32>,
}

static MONITOR: Mutex<Option<Monitor>> = Mutex::new(None);

pub(crate) fn snapshot() -> Result<Snapshot, String> {
    if crate::x11::is_wayland() {
        return Err("Clipboard event monitoring requires an X11 session".into());
    }
    let mut monitor = MONITOR
        .lock()
        .map_err(|_| "Clipboard monitor lock failed")?;
    if monitor.is_none() {
        *monitor = Some(Monitor::connect()?);
    }
    let result = monitor.as_mut().unwrap().snapshot();
    if result.is_err() {
        // A later action can reconnect after an X server disconnect.
        *monitor = None;
    }
    result
}

impl Monitor {
    fn connect() -> Result<Self, String> {
        let (connection, screen) =
            xcb::Connection::connect_with_extensions(None, &[], &[xcb::Extension::XFixes])
                .map_err(|e| e.to_string())?;
        if !connection
            .active_extensions()
            .any(|e| e == xcb::Extension::XFixes)
        {
            return Err("The X server does not provide XFixes clipboard events".into());
        }
        connection
            .wait_for_reply(connection.send_request(&xfixes::QueryVersion {
                client_major_version: 5,
                client_minor_version: 0,
            }))
            .map_err(|e| e.to_string())?;
        let root = connection
            .get_setup()
            .roots()
            .nth(screen as usize)
            .ok_or("X11 screen is unavailable")?
            .root();
        let window = connection.generate_id();
        connection
            .send_and_check_request(&x::CreateWindow {
                depth: 0,
                wid: window,
                parent: root,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                border_width: 0,
                class: x::WindowClass::InputOnly,
                visual: x::COPY_FROM_PARENT,
                value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
            })
            .map_err(|e| e.to_string())?;
        let atom = |name: &[u8]| {
            connection
                .wait_for_reply(connection.send_request(&x::InternAtom {
                    only_if_exists: false,
                    name,
                }))
                .map(|r| r.atom())
                .map_err(|e| e.to_string())
        };
        let selection = atom(b"CLIPBOARD")?;
        let clock_property = atom(b"_QUICKER_CLIPBOARD_CLOCK")?;
        connection
            .send_and_check_request(&xfixes::SelectSelectionInput {
                window,
                selection,
                event_mask: xfixes::SelectionEventMask::SET_SELECTION_OWNER
                    | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            connection,
            window,
            clock_property,
            sequence: 0,
            last_change: None,
        })
    }

    fn snapshot(&mut self) -> Result<Snapshot, String> {
        // PropertyNotify provides the server clock and an event queue barrier.
        // Use server timestamps so queued old events do not appear recent.
        self.connection.send_request(&x::ChangeProperty {
            mode: x::PropMode::Replace,
            window: self.window,
            property: self.clock_property,
            r#type: x::ATOM_INTEGER,
            data: &[0_u8],
        });
        self.connection.flush().map_err(|e| e.to_string())?;
        loop {
            match self
                .connection
                .wait_for_event()
                .map_err(|e| e.to_string())?
            {
                xcb::Event::XFixes(xfixes::Event::SelectionNotify(event)) => {
                    // Owner loss is a change as well. PRIMARY is not subscribed.
                    self.sequence = self.sequence.wrapping_add(1);
                    self.last_change = Some(event.timestamp());
                }
                xcb::Event::X(x::Event::PropertyNotify(event))
                    if event.window().resource_id() == self.window.resource_id()
                        && event.atom() == self.clock_property =>
                {
                    return Ok(Snapshot {
                        sequence: self.sequence,
                        age_ms: self.last_change.map(|time| event.time().wrapping_sub(time)),
                    });
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated X11 display with XFixes"]
    fn x11_clipboard_events_include_identical_copies_and_exclude_primary() {
        let mut monitor = Monitor::connect().unwrap();
        let (owner, screen) = xcb::Connection::connect(None).unwrap();
        let root = owner
            .get_setup()
            .roots()
            .nth(screen as usize)
            .unwrap()
            .root();
        let window = owner.generate_id();
        owner
            .send_and_check_request(&x::CreateWindow {
                depth: 0,
                wid: window,
                parent: root,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                border_width: 0,
                class: x::WindowClass::InputOnly,
                visual: x::COPY_FROM_PARENT,
                value_list: &[],
            })
            .unwrap();
        let clipboard = owner
            .wait_for_reply(owner.send_request(&x::InternAtom {
                only_if_exists: false,
                name: b"CLIPBOARD",
            }))
            .unwrap()
            .atom();
        let initial = monitor.snapshot().unwrap();
        assert_eq!(initial.age_ms, None);
        let set = |selection| {
            owner
                .send_and_check_request(&x::SetSelectionOwner {
                    owner: window,
                    selection,
                    time: x::CURRENT_TIME,
                })
                .unwrap()
        };
        set(x::ATOM_PRIMARY);
        assert_eq!(monitor.snapshot().unwrap().sequence, initial.sequence);
        set(clipboard);
        let first = monitor.snapshot().unwrap();
        assert_eq!(first.sequence, initial.sequence + 1);
        set(clipboard);
        let second = monitor.snapshot().unwrap();
        assert_eq!(second.sequence, first.sequence + 1);
        // Delay reading the event. The snapshot must retain the server event age.
        set(clipboard);
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(monitor.snapshot().unwrap().age_ms.unwrap() >= 30);
        owner
            .send_and_check_request(&x::DestroyWindow { window })
            .unwrap();
        assert_eq!(monitor.snapshot().unwrap().sequence, second.sequence + 2);
    }
}
