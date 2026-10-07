//! Bounded, cancellable X11 file clipboard transfers.
use super::Desktop;
use crate::action::ActionExecutionControl;
use std::time::{Duration, Instant};
use xcb::{x, Xid};

const MAX_BYTES: usize = 16 * 1024 * 1024;

struct Reader {
    desktop: Desktop,
    window: x::Window,
    clipboard: x::Atom,
    property: x::Atom,
    incr: x::Atom,
    owner: x::Window,
}

impl Reader {
    fn connect() -> Result<Self, String> {
        let desktop = Desktop::connect()?;
        let atom = |name: &[u8]| {
            desktop
                .conn
                .wait_for_reply(desktop.conn.send_request(&x::InternAtom {
                    only_if_exists: false,
                    name,
                }))
                .map(|r| r.atom())
                .map_err(|e| e.to_string())
        };
        let clipboard = atom(b"CLIPBOARD")?;
        let property = atom(b"_QUICKER_FILE_SELECTION")?;
        let incr = atom(b"INCR")?;
        let owner = desktop
            .conn
            .wait_for_reply(desktop.conn.send_request(&x::GetSelectionOwner {
                selection: clipboard,
            }))
            .map_err(|e| e.to_string())?
            .owner();
        if owner.is_none() {
            return Err("The file clipboard has no owner".into());
        }
        let window = desktop.conn.generate_id();
        desktop
            .conn
            .send_and_check_request(&x::CreateWindow {
                depth: 0,
                wid: window,
                parent: desktop.root,
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
        Ok(Self {
            desktop,
            window,
            clipboard,
            property,
            incr,
            owner,
        })
    }

    fn unchanged(&self) -> Result<(), String> {
        let owner = self
            .desktop
            .conn
            .wait_for_reply(self.desktop.conn.send_request(&x::GetSelectionOwner {
                selection: self.clipboard,
            }))
            .map_err(|e| e.to_string())?
            .owner();
        if owner != self.owner {
            return Err("The file clipboard changed during the read".into());
        }
        Ok(())
    }

    fn convert(
        &self,
        name: &[u8],
        deadline: Instant,
        control: Option<&ActionExecutionControl>,
    ) -> Result<Option<Vec<u8>>, String> {
        let conn = &self.desktop.conn;
        let target = conn
            .wait_for_reply(conn.send_request(&x::InternAtom {
                only_if_exists: false,
                name,
            }))
            .map_err(|e| e.to_string())?
            .atom();
        conn.send_and_check_request(&x::ConvertSelection {
            requestor: self.window,
            selection: self.clipboard,
            target,
            property: self.property,
            time: x::CURRENT_TIME,
        })
        .map_err(|e| e.to_string())?;
        let mut incremental = false;
        let mut bytes = Vec::new();
        loop {
            if control.is_some_and(ActionExecutionControl::is_cancelled) {
                return Err("Action cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("File clipboard transfer timed out".into());
            }
            let event = conn.poll_for_event().map_err(|e| e.to_string())?;
            let ready = match event {
                Some(xcb::Event::X(x::Event::SelectionNotify(event)))
                    if event.requestor() == self.window
                        && event.selection() == self.clipboard
                        && event.target() == target =>
                {
                    if event.property().is_none() {
                        return Ok(None);
                    }
                    if event.property() != self.property {
                        return Err("Invalid file clipboard response property".into());
                    }
                    true
                }
                Some(xcb::Event::X(x::Event::PropertyNotify(event))) => {
                    incremental
                        && event.window() == self.window
                        && event.atom() == self.property
                        && event.state() == x::Property::NewValue
                }
                None => {
                    std::thread::sleep(Duration::from_millis(2));
                    false
                }
                _ => false,
            };
            if !ready {
                continue;
            }
            self.unchanged()?;
            let reply = conn
                .wait_for_reply(conn.send_request(&x::GetProperty {
                    delete: true,
                    window: self.window,
                    property: self.property,
                    r#type: x::ATOM_ANY,
                    long_offset: 0,
                    long_length: (MAX_BYTES / 4 + 1) as u32,
                }))
                .map_err(|e| e.to_string())?;
            if reply.bytes_after() > 0 {
                return Err("File clipboard exceeds 16 MiB".into());
            }
            if reply.r#type() == self.incr && !incremental {
                if reply.format() != 32
                    || reply.value::<u32>().len() != 1
                    || reply.value::<u32>()[0] as usize > MAX_BYTES
                {
                    return Err("Invalid or oversized incremental file clipboard".into());
                }
                incremental = true;
                continue;
            }
            if reply.r#type() != target || reply.format() != 8 {
                return Err("Invalid file clipboard format".into());
            }
            let chunk = reply.value::<u8>();
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err("File clipboard exceeds 16 MiB".into());
            }
            bytes.extend_from_slice(chunk);
            if !incremental || chunk.is_empty() {
                self.unchanged()?;
                return Ok(Some(bytes));
            }
        }
    }
}

pub(crate) fn read_file_selection(
    control: Option<&ActionExecutionControl>,
) -> Result<Vec<u8>, String> {
    if super::is_wayland() {
        return Err("File selection requires an X11 session".into());
    }
    let reader = Reader::connect()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    if let Some(bytes) = reader.convert(b"text/uri-list", deadline, control)? {
        return Ok(bytes);
    }
    if let Some(bytes) = reader.convert(b"x-special/gnome-copied-files", deadline, control)? {
        let newline = bytes
            .iter()
            .position(|c| *c == b'\n')
            .ok_or("Invalid GNOME file clipboard")?;
        if !matches!(&bytes[..newline], b"copy" | b"cut") {
            return Err("Invalid GNOME file clipboard operation".into());
        }
        return Ok(bytes[newline + 1..].to_vec());
    }
    Err("The active application did not copy a file list".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };

    #[derive(Clone, Copy)]
    enum Mode {
        Plain,
        Incr,
        Gnome,
        Unsupported,
        Oversized,
        Silent,
        ChangeOwner,
    }

    struct Owner {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.thread.take().unwrap().join().unwrap();
        }
    }
    fn owner(payload: Vec<u8>, mode: Mode) -> Owner {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let desktop = Desktop::connect().unwrap();
            let conn = &desktop.conn;
            let atom = |name: &[u8]| {
                conn.wait_for_reply(conn.send_request(&x::InternAtom {
                    only_if_exists: false,
                    name,
                }))
                .unwrap()
                .atom()
            };
            let clipboard = atom(b"CLIPBOARD");
            let target = atom(if matches!(mode, Mode::Gnome) {
                b"x-special/gnome-copied-files"
            } else {
                b"text/uri-list"
            });
            let incr = atom(b"INCR");
            let window = conn.generate_id();
            conn.send_and_check_request(&x::CreateWindow {
                depth: 0,
                wid: window,
                parent: desktop.root,
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
            conn.send_and_check_request(&x::SetSelectionOwner {
                owner: window,
                selection: clipboard,
                time: x::CURRENT_TIME,
            })
            .unwrap();
            tx.send(()).unwrap();
            let mut transfer: Option<(x::Window, x::Atom, usize)> = None;
            while !stopped.load(Ordering::SeqCst) {
                match conn.poll_for_event().unwrap() {
                    Some(xcb::Event::X(x::Event::SelectionRequest(e))) => {
                        if matches!(mode, Mode::Silent) {
                            continue;
                        }
                        let supported = e.target() == target && !matches!(mode, Mode::Unsupported);
                        if supported {
                            if matches!(mode, Mode::Incr | Mode::Oversized) {
                                conn.send_and_check_request(&x::ChangeWindowAttributes {
                                    window: e.requestor(),
                                    value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
                                })
                                .unwrap();
                                let size = if matches!(mode, Mode::Oversized) {
                                    MAX_BYTES as u32 + 1
                                } else {
                                    payload.len() as u32
                                };
                                conn.send_request(&x::ChangeProperty {
                                    mode: x::PropMode::Replace,
                                    window: e.requestor(),
                                    property: e.property(),
                                    r#type: incr,
                                    data: &[size],
                                });
                                if matches!(mode, Mode::Incr) {
                                    transfer = Some((e.requestor(), e.property(), 0));
                                }
                            } else {
                                conn.send_request(&x::ChangeProperty {
                                    mode: x::PropMode::Replace,
                                    window: e.requestor(),
                                    property: e.property(),
                                    r#type: target,
                                    data: &payload,
                                });
                            }
                        }
                        if matches!(mode, Mode::ChangeOwner) {
                            conn.send_request(&x::SetSelectionOwner {
                                owner: x::Window::none(),
                                selection: clipboard,
                                time: x::CURRENT_TIME,
                            });
                        }
                        let notify = x::SelectionNotifyEvent::new(
                            e.time(),
                            e.requestor(),
                            e.selection(),
                            e.target(),
                            if supported {
                                e.property()
                            } else {
                                x::Atom::none()
                            },
                        );
                        conn.send_request(&x::SendEvent {
                            propagate: false,
                            destination: x::SendEventDest::Window(e.requestor()),
                            event_mask: x::EventMask::NO_EVENT,
                            event: &notify,
                        });
                        conn.flush().unwrap();
                    }
                    Some(xcb::Event::X(x::Event::PropertyNotify(e))) => {
                        if let Some((window, property, offset)) = transfer {
                            if e.window() == window
                                && e.atom() == property
                                && e.state() == x::Property::Delete
                            {
                                let end = (offset + 4096).min(payload.len());
                                conn.send_request(&x::ChangeProperty {
                                    mode: x::PropMode::Replace,
                                    window,
                                    property,
                                    r#type: target,
                                    data: &payload[offset..end],
                                });
                                conn.flush().unwrap();
                                transfer = (offset != end).then_some((window, property, end));
                            }
                        }
                    }
                    None => std::thread::sleep(Duration::from_millis(1)),
                    _ => {}
                }
            }
        });
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        Owner {
            stop,
            thread: Some(thread),
        }
    }

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn file_selection_transfers_validate_incr_fallback_limits_and_cancellation() {
        for mode in [Mode::Plain, Mode::Incr, Mode::Gnome] {
            let expected = b"file:///tmp/a%20b\r\n".repeat(5000);
            let payload = if matches!(mode, Mode::Gnome) {
                [b"copy\n".as_slice(), &expected].concat()
            } else {
                expected.clone()
            };
            let _owner = owner(payload, mode);
            assert_eq!(read_file_selection(None).unwrap(), expected);
        }
        for (mode, error) in [
            (Mode::Unsupported, "did not copy"),
            (Mode::Oversized, "oversized"),
            (Mode::ChangeOwner, "changed"),
        ] {
            let _owner = owner(b"file:///tmp/a".to_vec(), mode);
            assert!(read_file_selection(None).unwrap_err().contains(error));
        }
        let _owner = owner(vec![], Mode::Silent);
        let reader = Reader::connect().unwrap();
        assert!(reader
            .convert(
                b"text/uri-list",
                Instant::now() + Duration::from_millis(30),
                None
            )
            .unwrap_err()
            .contains("timed out"));
        let control = ActionExecutionControl::new();
        let cancel = control.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cancel.cancel();
        });
        let start = Instant::now();
        assert!(read_file_selection(Some(&control))
            .unwrap_err()
            .contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(1));
        thread.join().unwrap();
    }
}
