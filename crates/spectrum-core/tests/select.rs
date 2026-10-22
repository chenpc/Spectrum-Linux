use spectrum_core::select::{apply_click, ClickMods, Selection};

const NONE: ClickMods = ClickMods {
    ctrl: false,
    shift: false,
};
const CTRL: ClickMods = ClickMods {
    ctrl: true,
    shift: false,
};
const SHIFT: ClickMods = ClickMods {
    ctrl: false,
    shift: true,
};

#[test]
fn plain_click_replaces_the_selection() {
    let sel = apply_click(&Selection::default(), 2, NONE);
    assert_eq!(sel.count(), 1);
    assert_eq!(sel.set, vec![2usize]);
    assert_eq!(sel.primary, Some(2));

    let sel = apply_click(&sel, 5, NONE);
    assert_eq!(sel.set, vec![5usize]);
}

#[test]
fn ctrl_click_adds_and_toggles_off() {
    let sel = apply_click(&Selection::default(), 1, NONE);
    let sel = apply_click(&sel, 3, CTRL);
    assert_eq!(sel.set, vec![1usize, 3usize]);

    // clicking an already-selected row removes it, and the anchor moves to the clicked row
    let sel = apply_click(&sel, 3, CTRL);
    assert_eq!(sel.set, vec![1usize]);
    assert_eq!(sel.primary, Some(3));

    // the anchor is kept even when the anchor itself is toggled off
    let sel = apply_click(&sel, 1, CTRL);
    assert_eq!(sel.set, Vec::<usize>::new());
    assert_eq!(sel.primary, Some(1));
}

#[test]
fn shift_selects_the_range_from_the_anchor_inclusive() {
    let sel = apply_click(&Selection::default(), 2, NONE);
    let sel = apply_click(&sel, 5, SHIFT);
    assert_eq!(sel.set, vec![2usize, 3usize, 4usize, 5usize]);

    // backwards range
    let sel = apply_click(&sel, 1, SHIFT);
    assert_eq!(sel.set, vec![1usize, 2usize]);
    assert_eq!(sel.count(), 2);
}

#[test]
fn shift_without_an_anchor_selects_only_the_clicked_row() {
    let sel = apply_click(&Selection::default(), 4, SHIFT);
    assert_eq!(sel.set, vec![4usize]);
}

#[test]
fn shift_replaces_the_set_instead_of_merging() {
    let sel = apply_click(&Selection::default(), 0, NONE);
    let sel = apply_click(&sel, 2, CTRL);
    assert_eq!(sel.set, vec![0usize, 2usize]);
    // anchor is 2, so Shift from 2 to 4 gives 2..4 and drops the earlier 0
    let sel = apply_click(&sel, 4, SHIFT);
    assert_eq!(sel.set, vec![2usize, 3usize, 4usize]);
}

#[test]
fn set_is_always_sorted_and_unique() {
    let mut sel = Selection::default();
    for i in [7, 3, 9, 3] {
        sel = apply_click(&sel, i, CTRL);
    }
    // Ctrl-click toggles, so the repeated 3 is removed on the second click
    assert_eq!(sel.set, vec![7usize, 9usize]);
    assert!(sel.contains(7) && !sel.contains(4));
}
