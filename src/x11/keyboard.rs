//! Single-key state and injection. Each action owns the keys it presses.
use xcb::{x, xkb, xtest};

pub(crate) struct Keyboard {
    connection: xcb::Connection,
    root: x::Window,
    held: Vec<u8>,
}

impl Keyboard {
    pub(crate) fn connect() -> Result<Self, String> {
        if super::is_wayland() {
            return Err("Key operations require an X11 session".into());
        }
        let (connection, screen) = xcb::Connection::connect_with_extensions(
            None,
            &[],
            &[xcb::Extension::Test, xcb::Extension::Xkb],
        )
        .map_err(|e| e.to_string())?;
        for extension in [xcb::Extension::Test, xcb::Extension::Xkb] {
            if !connection.active_extensions().any(|e| e == extension) {
                return Err("The X server must provide XTEST and XKB for key operations".into());
            }
        }
        let version = connection
            .wait_for_reply(connection.send_request(&xkb::UseExtension {
                wanted_major: 1,
                wanted_minor: 0,
            }))
            .map_err(|e| e.to_string())?;
        if !version.supported() {
            return Err("The X server does not support XKB 1.0".into());
        }
        connection
            .wait_for_reply(connection.send_request(&xtest::GetVersion {
                major_version: 2,
                minor_version: 2,
            }))
            .map_err(|e| e.to_string())?;
        let root = connection
            .get_setup()
            .roots()
            .nth(screen as usize)
            .ok_or("X11 screen is unavailable")?
            .root();
        Ok(Self {
            connection,
            root,
            held: vec![],
        })
    }

    fn keycodes(&self, symbols: &[u32]) -> Result<Vec<u8>, String> {
        let setup = self.connection.get_setup();
        let first = setup.min_keycode();
        let count = setup.max_keycode() - first + 1;
        let reply = self
            .connection
            .wait_for_reply(self.connection.send_request(&x::GetKeyboardMapping {
                first_keycode: first,
                count,
            }))
            .map_err(|e| e.to_string())?;
        let width = usize::from(reply.keysyms_per_keycode());
        if width == 0 {
            return Err("The X11 keyboard mapping is empty".into());
        }
        let mut result = Vec::new();
        for symbol in symbols {
            for (index, row) in reply.keysyms().chunks_exact(width).enumerate() {
                if row.contains(symbol) {
                    let code = first + index as u8;
                    if !result.contains(&code) {
                        result.push(code);
                    }
                }
            }
        }
        if result.is_empty() {
            return Err("The requested key is not mapped in the X11 keyboard layout".into());
        }
        Ok(result)
    }

    fn pressed(&self) -> Result<[u8; 32], String> {
        let reply = self
            .connection
            .wait_for_reply(self.connection.send_request(&x::QueryKeymap {}))
            .map_err(|e| e.to_string())?;
        Ok(*reply.keys())
    }

    pub(crate) fn state(&self, key: u16, symbols: &[u32]) -> Result<(bool, bool), String> {
        if matches!(key, 1 | 2 | 4) {
            let reply = self
                .connection
                .wait_for_reply(
                    self.connection
                        .send_request(&x::QueryPointer { window: self.root }),
                )
                .map_err(|e| e.to_string())?;
            let mask = match key {
                1 => x::KeyButMask::BUTTON1,
                2 => x::KeyButMask::BUTTON3,
                _ => x::KeyButMask::BUTTON2,
            };
            return Ok((reply.mask().contains(mask), false));
        }
        if matches!(key, 5 | 6) {
            return Err(
                "Side mouse button state is not available through the X11 core keyboard backend"
                    .into(),
            );
        }
        let codes = self.keycodes(symbols)?;
        let pressed = self.pressed()?;
        let down = codes.iter().any(|code| is_pressed(&pressed, *code));
        let mut toggled = false;
        if matches!(key, 20 | 144 | 145) {
            let state = self
                .connection
                .wait_for_reply(self.connection.send_request(&xkb::GetState {
                    device_spec: 0x100, // XkbUseCoreKbd
                }))
                .map_err(|e| e.to_string())?;
            let modifiers = self
                .connection
                .wait_for_reply(self.connection.send_request(&x::GetModifierMapping {}))
                .map_err(|e| e.to_string())?;
            let width = modifiers.keycodes().len() / 8;
            if width > 0 {
                for (index, row) in modifiers.keycodes().chunks_exact(width).enumerate() {
                    if row.iter().any(|code| codes.contains(code))
                        && state.locked_mods().bits() & (1 << index) != 0
                    {
                        toggled = true;
                    }
                }
            }
        }
        Ok((down, toggled))
    }

    pub(crate) fn change(&mut self, key: u16, symbols: &[u32], down: bool) -> Result<(), String> {
        if matches!(key, 1 | 2 | 4 | 5 | 6) {
            return Err("Mouse key injection requires a mouse input module".into());
        }
        let codes = self.keycodes(symbols)?;
        if down {
            let code = codes[0];
            // Do not claim a key which was already held by another input source.
            if !is_pressed(&self.pressed()?, code) && !self.held.contains(&code) {
                self.held.push(code);
            }
            self.event(code, true)?;
        } else {
            for code in codes {
                self.event(code, false)?;
                self.held.retain(|held| *held != code);
            }
        }
        Ok(())
    }

    fn event(&self, code: u8, down: bool) -> Result<(), String> {
        self.connection
            .send_and_check_request(&xtest::FakeInput {
                r#type: if down { 2 } else { 3 },
                detail: code,
                time: x::CURRENT_TIME,
                root: self.root,
                root_x: 0,
                root_y: 0,
                deviceid: 0,
            })
            .map_err(|e| e.to_string())
    }
}

fn is_pressed(keys: &[u8; 32], code: u8) -> bool {
    keys[usize::from(code / 8)] & (1 << (code % 8)) != 0
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        for code in self.held.iter().rev() {
            if let Err(error) = self.event(*code, false) {
                log::warn!("Could not release action key {code}: {error}");
            }
        }
    }
}
