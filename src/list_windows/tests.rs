use super::*;

fn model() -> Model {
    Model::new(Options {
        items: ["a", "dup", "c", "dup", "末尾  "]
            .map(String::from)
            .to_vec(),
        title: "List".into(),
        note: String::new(),
        width: 640.0,
        allow_add: true,
        allow_edit: true,
        allow_delete: true,
    })
}

#[test]
fn moving_nonadjacent_duplicate_items_keeps_identity_and_order() {
    let mut m = model();
    m.select(1, false, false);
    m.select(3, true, false);
    m.move_to(5);
    assert_eq!(m.items, ["a", "c", "末尾  ", "dup", "dup"]);
    assert_eq!(m.selected, BTreeSet::from([3, 4]));
    m.move_to(0);
    assert_eq!(m.items, ["dup", "dup", "a", "c", "末尾  "]);
    m.delete();
    assert_eq!(m.items, ["a", "c", "末尾  "]);
    m.reset();
    assert_eq!(m.items, m.options.items);
}

#[test]
fn range_selection_add_position_required_value_and_permissions() {
    let mut m = model();
    m.select(1, false, false);
    m.select(3, false, true);
    assert_eq!(m.selected, BTreeSet::from([1, 2, 3]));
    m.edit(false);
    assert!(m.editor.is_none());
    m.edit(true);
    assert!(m.apply_edit().is_err());
    m.editor.as_mut().unwrap().text = " ".into();
    m.apply_edit().unwrap();
    assert_eq!(m.items, ["a", "dup", " ", "c", "dup", "末尾  "]);
    m.options.allow_add = false;
    m.options.allow_edit = false;
    m.options.allow_delete = false;
    let before = m.items.clone();
    m.edit(true);
    m.edit(false);
    m.delete();
    assert_eq!(m.items, before);
    assert!(m.editor.is_none());
    m.sort(true);
    assert_eq!(m.items[0], "末尾  ");
}
