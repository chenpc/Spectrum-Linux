use std::path::{Path, PathBuf};

use spectrum_core::edit::EditOp;
use spectrum_core::undo::{Change, UndoStack};

fn crop() -> EditOp {
    EditOp::Crop(spectrum_core::crop::CropRect {
        x: 0.2,
        y: 0.1,
        width: 0.6,
        height: 0.8,
    })
}

#[test]
fn record_pushes_and_undoes_to_the_previous_ops() {
    let mut stack = UndoStack::new();
    assert!(!stack.can_undo());

    stack.record(Change::new(
        PathBuf::from("a.png"),
        vec![EditOp::Rotate(90)],
        EditOp::FlipH,
    ));
    assert!(stack.can_undo());

    let change = stack.undo().unwrap();
    assert_eq!(change.ops_before, vec![EditOp::Rotate(90)]);
    assert_eq!(change.ops_after, vec![EditOp::Rotate(90), EditOp::FlipH]);
    // the change is now available for redo
    assert!(!stack.can_undo());
    assert!(stack.can_redo());
}

#[test]
fn redo_moves_the_change_back_to_the_undo_stack() {
    let mut stack = UndoStack::new();
    stack.record(Change::new(PathBuf::from("a.png"), vec![], EditOp::FlipH));

    stack.undo();
    let change = stack.redo().unwrap();
    assert_eq!(change.ops_after, vec![EditOp::FlipH]);
    assert!(stack.can_undo());
    assert!(!stack.can_redo());

    // undoing again returns the same change
    let again = stack.undo().unwrap();
    assert_eq!(again, change);
}

#[test]
fn a_new_change_clears_the_redo_stack() {
    let mut stack = UndoStack::new();
    stack.record(Change::new(
        PathBuf::from("a.png"),
        vec![],
        EditOp::Rotate(90),
    ));
    stack.record(Change::new(
        PathBuf::from("a.png"),
        vec![EditOp::Rotate(90)],
        crop(),
    ));
    stack.undo();
    assert_eq!(stack.redo_len(), 1);

    // a fresh edit invalidates the undone branch
    stack.record(Change::new(PathBuf::from("a.png"), vec![], EditOp::FlipH));
    assert_eq!(stack.redo_len(), 0);
    // the earlier Rotate(90) change is still undoable; only the undone branch was dropped
    assert_eq!(stack.undo_len(), 2);
    assert_eq!(stack.undo().unwrap().ops_after, vec![EditOp::FlipH]);
}

#[test]
fn changes_are_undone_in_reverse_order() {
    let mut stack = UndoStack::new();
    stack.record(Change::new(
        PathBuf::from("a.png"),
        vec![],
        EditOp::Rotate(90),
    ));
    stack.record(Change::new(
        PathBuf::from("a.png"),
        vec![EditOp::Rotate(90)],
        EditOp::FlipH,
    ));
    stack.record(Change::new(
        PathBuf::from("a.png"),
        vec![EditOp::Rotate(90), EditOp::FlipH],
        crop(),
    ));

    assert_eq!(
        stack.undo().unwrap().ops_before,
        vec![EditOp::Rotate(90), EditOp::FlipH]
    );
    assert_eq!(stack.undo().unwrap().ops_before, vec![EditOp::Rotate(90)]);
    assert_eq!(stack.undo().unwrap().ops_before, vec![]);
    assert_eq!(stack.undo(), None);
}

#[test]
fn a_no_op_is_not_recorded() {
    let mut stack = UndoStack::new();
    // before == after => nothing changed, so nothing is recorded
    assert!(!stack.record(Change {
        path: PathBuf::from("a.png"),
        ops_before: vec![EditOp::FlipH],
        ops_after: vec![EditOp::FlipH],
    }));
    assert!(!stack.can_undo());

    // a real change is recorded
    assert!(stack.record(Change::new(PathBuf::from("a.png"), vec![], EditOp::FlipH)));
    assert!(stack.can_undo());
}

#[test]
fn apply_records_and_returns_the_new_ops() {
    let mut stack = UndoStack::new();
    let ops = stack.apply(Path::new("a.png"), &[], EditOp::Rotate(90));
    assert_eq!(ops, vec![EditOp::Rotate(90)]);
    let ops = stack.apply(Path::new("a.png"), &ops, EditOp::FlipH);
    assert_eq!(ops, vec![EditOp::Rotate(90), EditOp::FlipH]);
    assert_eq!(stack.undo_len(), 2);

    // a change recorded for another file keeps its own path
    let ops = stack.apply(Path::new("b.png"), &[], EditOp::FlipH);
    assert_eq!(stack.undo().unwrap().path, PathBuf::from("b.png"));
    assert_eq!(ops, vec![EditOp::FlipH]);
}

#[test]
fn clear_empties_both_stacks() {
    let mut stack = UndoStack::new();
    stack.record(Change::new(PathBuf::from("a.png"), vec![], EditOp::FlipH));
    stack.undo();
    assert!(!stack.is_empty());
    stack.clear();
    assert!(stack.is_empty());
    assert!(!stack.can_undo() && !stack.can_redo());
}
