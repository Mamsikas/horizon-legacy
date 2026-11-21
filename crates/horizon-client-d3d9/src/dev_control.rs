//! A dev hotkey fires once per press, and an out-of-focus press cannot fire on
//! focus gain. No keyboard state is stored beyond this single chord bit.
pub(crate) fn pressed_edge(held: &mut bool, down: bool, focused: bool) -> bool {
    let pressed = down && !*held && focused;
    *held = down;
    pressed
}

#[cfg(test)]
mod tests {
    #[test]
    fn hold_and_focus_changes_do_not_repeat_a_toggle() {
        let mut held = false;
        assert!(!super::pressed_edge(&mut held, false, true));
        assert!(super::pressed_edge(&mut held, true, true));
        assert!(!super::pressed_edge(&mut held, true, true));
        assert!(!super::pressed_edge(&mut held, false, false));
        assert!(!super::pressed_edge(&mut held, true, false));
        assert!(!super::pressed_edge(&mut held, true, true));
        assert!(!super::pressed_edge(&mut held, false, true));
        assert!(super::pressed_edge(&mut held, true, true));
    }
}
