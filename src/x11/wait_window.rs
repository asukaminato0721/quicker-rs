//! Configure waiting viewports before X11 maps them.
use super::*;

fn atom(desktop: &Desktop, name: &[u8]) -> Result<x::Atom, String> {
    desktop
        .conn
        .wait_for_reply(desktop.conn.send_request(&x::InternAtom {
            only_if_exists: false,
            name,
        }))
        .map(|r| r.atom())
        .map_err(|e| e.to_string())
}

pub(crate) fn configure(title: &str, activation: &str) -> Result<Option<u32>, String> {
    let desktop = Desktop::connect()?;
    let children = desktop
        .conn
        .wait_for_reply(desktop.conn.send_request(&x::QueryTree {
            window: desktop.root,
        }))
        .map_err(|e| e.to_string())?;
    for window in children.children() {
        if desktop.pid(*window) != std::process::id() {
            continue;
        }
        let name = desktop.property(*window, desktop.atoms.title, desktop.atoms.utf8)?;
        if name.format() != 8 || name.value::<u8>() != title.as_bytes() {
            continue;
        }
        if activation != "AutoActivate" {
            desktop
                .conn
                .send_and_check_request(&x::ChangeProperty {
                    mode: x::PropMode::Replace,
                    window: *window,
                    property: atom(&desktop, b"_NET_WM_USER_TIME")?,
                    r#type: x::ATOM_CARDINAL,
                    data: &[0u32],
                })
                .map_err(|e| e.to_string())?;
        }
        if activation == "NotActivatable" {
            let hints = atom(&desktop, b"WM_HINTS")?;
            let old = desktop.property(*window, hints, hints)?;
            let mut values = if old.format() == 32 {
                old.value::<u32>().to_vec()
            } else {
                Vec::new()
            };
            values.resize(9, 0);
            values[0] |= 1; // ICCCM InputHint.
            values[1] = 0;
            desktop
                .conn
                .send_and_check_request(&x::ChangeProperty {
                    mode: x::PropMode::Replace,
                    window: *window,
                    property: hints,
                    r#type: hints,
                    data: &values,
                })
                .map_err(|e| e.to_string())?;
            let protocols = atom(&desktop, b"WM_PROTOCOLS")?;
            let take_focus = atom(&desktop, b"WM_TAKE_FOCUS")?;
            let old = desktop.property(*window, protocols, x::ATOM_ATOM)?;
            let values: Vec<x::Atom> = if old.format() == 32 {
                old.value::<x::Atom>()
                    .iter()
                    .copied()
                    .filter(|a| *a != take_focus)
                    .collect()
            } else {
                Vec::new()
            };
            desktop
                .conn
                .send_and_check_request(&x::ChangeProperty {
                    mode: x::PropMode::Replace,
                    window: *window,
                    property: protocols,
                    r#type: x::ATOM_ATOM,
                    data: &values,
                })
                .map_err(|e| e.to_string())?;
        }
        desktop.conn.flush().map_err(|e| e.to_string())?;
        return Ok(Some(window.resource_id()));
    }
    Ok(None)
}

pub(crate) fn position(window: u32, scale: f32) -> Result<egui::Pos2, String> {
    let desktop = Desktop::connect()?;
    let window = x::Window::new(window);
    if desktop.pid(window) != std::process::id() {
        return Err("The wait window no longer belongs to this process".into());
    }
    let origin = desktop
        .conn
        .wait_for_reply(desktop.conn.send_request(&x::TranslateCoordinates {
            src_window: window,
            dst_window: desktop.root,
            src_x: 0,
            src_y: 0,
        }))
        .map_err(|e| e.to_string())?;
    let frame = desktop.property(
        window,
        atom(&desktop, b"_NET_FRAME_EXTENTS")?,
        x::ATOM_CARDINAL,
    )?;
    let (left, top) = if frame.format() == 32 && frame.value::<u32>().len() >= 4 {
        (
            frame.value::<u32>()[0] as f32,
            frame.value::<u32>()[2] as f32,
        )
    } else {
        (0.0, 0.0)
    };
    Ok(egui::pos2(
        (origin.dst_x() as f32 - left) / scale,
        (origin.dst_y() as f32 - top) / scale,
    ))
}

pub(crate) fn geometry(scale: f32) -> Result<(egui::Rect, egui::Pos2), String> {
    let desktop = Desktop::connect()?;
    let root = desktop
        .conn
        .wait_for_reply(desktop.conn.send_request(&x::GetGeometry {
            drawable: x::Drawable::Window(desktop.root),
        }))
        .map_err(|e| e.to_string())?;
    let pointer = desktop
        .conn
        .wait_for_reply(desktop.conn.send_request(&x::QueryPointer {
            window: desktop.root,
        }))
        .map_err(|e| e.to_string())?;
    let mut area = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(root.width() as f32, root.height() as f32),
    );
    let current = desktop.property(
        desktop.root,
        atom(&desktop, b"_NET_CURRENT_DESKTOP")?,
        x::ATOM_CARDINAL,
    )?;
    let current = if current.format() == 32 {
        current.value::<u32>().first().copied().unwrap_or(0) as usize
    } else {
        0
    };
    let work = desktop.property(
        desktop.root,
        atom(&desktop, b"_NET_WORKAREA")?,
        x::ATOM_CARDINAL,
    )?;
    if work.format() == 32 {
        if let Some(rect) = current
            .checked_mul(4)
            .and_then(|start| work.value::<u32>().get(start..start.saturating_add(4)))
        {
            if rect[2] > 0 && rect[3] > 0 {
                area = egui::Rect::from_min_size(
                    egui::pos2(rect[0] as i32 as f32, rect[1] as i32 as f32),
                    egui::vec2(rect[2] as f32, rect[3] as f32),
                );
            }
        }
    }
    Ok((
        egui::Rect::from_min_max(area.min / scale, area.max / scale),
        egui::pos2(
            pointer.root_x() as f32 / scale,
            pointer.root_y() as f32 / scale,
        ),
    ))
}
