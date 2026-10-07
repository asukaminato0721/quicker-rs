use super::*;

fn options(key: &str, text: &str) -> Options {
    Options {
        title: "Test".into(),
        text: text.into(),
        key: key.into(),
        size: [720.0, 480.0],
        font_size: 14.0,
        top_most: false,
        wrap: true,
        line_numbers: true,
        toolbar: true,
        escape_close: true,
        close_on_blur: false,
        caret: 0,
        background: None,
        foreground: None,
        operations: vec![],
    }
}

#[test]
fn replacement_finishes_old_waiters_and_update_keeps_existing_window() {
    let mut registry = Registry::default();
    let old = registry.open(options("key", "old"), false).unwrap();
    let new = registry.open(options("key", "new"), false).unwrap();
    assert!(snapshot(&old).closed);
    assert!(!snapshot(&new).closed);
    assert_ne!(old.lock().unwrap().id, new.lock().unwrap().id);
    let updated = registry.open(options("key", "更新\r\n😀"), true).unwrap();
    assert!(Arc::ptr_eq(&new, &updated));
    assert_eq!(snapshot(&updated).text, "更新\r\n😀");
    assert!(updated.lock().unwrap().reset_cursor);
    assert_eq!(registry.windows.len(), 1);
    let unnamed = registry.open(options("", "first"), false).unwrap();
    registry.open(options("", "second"), false).unwrap();
    assert!(!snapshot(&unnamed).closed);
    assert_eq!(registry.windows.len(), 3);
}

#[test]
fn append_is_exact_and_limits_do_not_damage_existing_windows() {
    let host = Host {
        registry: Default::default(),
        context: Context::default(),
    };
    let handle = host.open(options("key", "a \r\n"), false).unwrap();
    host.append(&handle, "\t中文😀\n").unwrap();
    let expected = snapshot(&handle).text;
    assert_eq!(expected, "a \r\n\t中文😀\n");
    assert!(host.append(&handle, &"x".repeat(TEXT_LIMIT)).is_err());
    assert_eq!(snapshot(&handle).text, expected);
    assert!(host
        .open(options("key", &"x".repeat(TEXT_LIMIT + 1)), false)
        .is_err());
    assert!(!snapshot(&handle).closed);
    host.close(&handle);
    assert!(host.find("key").unwrap().is_none());
    assert!(host.find("").is_err());
}

#[test]
fn caret_uses_utf16_offsets_and_clamps_to_character_boundaries() {
    for (offset, index) in [(0, 0), (1, 1), (2, 1), (3, 2), (4, 3), (99, 3), (-1, 3)] {
        assert_eq!(char_index("a😀中", offset), index);
    }
}

#[test]
fn open_windows_are_bounded_and_closed_documents_are_released() {
    let mut registry = Registry::default();
    for n in 0..32 {
        registry.open(options(&n.to_string(), "x"), false).unwrap();
    }
    assert!(registry.open(options("overflow", "x"), false).is_err());
    let first = registry.find("0").unwrap();
    first.lock().unwrap().result.closed = true;
    registry.open(options("next", "x"), false).unwrap();
    assert_eq!(registry.windows.len(), 32);
}
