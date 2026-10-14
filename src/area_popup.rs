//! A popup anchored to a widget that SURVIVES clicks inside itself.
//!
//! `egui::ComboBox` is built on `containers/popup.rs`, whose rule is "close when the
//! widget that owns the popup reports `clicked_elsewhere()`" - and a press on a
//! `TextEdit` *inside* that popup counts as elsewhere. That is the bug behind
//! "clicking the search box makes the menu disappear": any popup carrying an input
//! field closes the moment you try to use it.
//!
//! The fix is the one applied to [`crate::searchable_combo`]: own the popup with an
//! `Area` in the foreground order and decide dismissal yourself. This is the small,
//! reusable form of that for popups which are not a filtered list - the PP ladder
//! picker, for instance - where the content is arbitrary widgets and may include an
//! editable field.
//!
//! Contract:
//! * opens on the caller's request (normally `button.clicked()`);
//! * stays open while clicks land inside its own rect or on the anchor;
//! * closes on a press that starts outside both, on Escape, or on a second anchor press;
//! * as an `Area` in `Order::Foreground` it is not clipped by the `ScrollArea` the
//!   trigger sits in (a `ComboBox` inside a scrolled page is).

use egui::{Area, Context, Frame, Id, Key, Order, PointerButton, Rect, Response, Ui};

#[derive(Clone)]
struct State {
    open: bool,
    /// Rect the content occupied last frame: the "inside" test region.
    rect: Rect,
    /// Frame the popup opened on, so the opening press is not also read as a
    /// dismissal (the pointer is still down during that frame).
    opened_frame: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            rect: Rect::NOTHING,
            opened_frame: 0,
        }
    }
}

/// Draw `content` in a popup anchored under (or above, when there is no room)
/// `anchor`. Returns `Some(value)` for every frame the popup is open, so the caller
/// can collect what the content produced; the content runs in its own `Ui`.
pub fn anchored<T>(
    ui: &mut Ui,
    id: &str,
    anchor: &Response,
    open_request: bool,
    content: impl FnOnce(&mut Ui) -> T,
) -> Option<T> {
    let ctx = ui.ctx().clone();
    let state_id = Id::new(id);
    let frame = ctx.frame_nr();

    let mut st: State = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
    if open_request {
        // A click on the anchor toggles: open it, or close an already-open popup.
        st.open = !st.open || st.opened_frame != frame;
        st.opened_frame = frame;
    }
    if !st.open {
        st.rect = Rect::NOTHING;
        ctx.data_mut(|d| d.insert_temp(state_id, st));
        return None;
    }

    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    let pressed = ctx.input(|i| i.pointer.button_pressed(PointerButton::Primary));
    let press_at = ctx.input(|i| i.pointer.interact_pos());
    let outside = match press_at {
        Some(p) => !anchor.rect.contains(p) && !st.rect.contains(p),
        None => false,
    };
    if escape || (pressed && outside && st.opened_frame != frame) {
        st.open = false;
        st.rect = Rect::NOTHING;
        ctx.data_mut(|d| d.insert_temp(state_id, st));
        return None;
    }

    // Flip above the anchor when there is no room below, so a popup near the bottom
    // of a scrolled page is still reachable.
    let height = st.rect.height().max(0.0);
    let screen = ctx.input(|i| i.screen_rect);
    let below = anchor.rect.left_bottom();
    let above = anchor.rect.left_top() - egui::vec2(0.0, height);
    let pos = if below.y + height > screen.bottom() && above.y > screen.top() {
        above
    } else {
        below
    };

    // The rect that matters is the FRAME's, not the Area's: an `Area` reports no
    // rect of its own, so hit-testing against it reads as "every click is outside"
    // and the backdrop never gets drawn. `Frame::show` gives a real rect, which is
    // also the visible background the popup needs.
    let inner = Area::new(state_id.with("area"))
        .order(Order::Foreground)
        .constrain(true)
        .default_pos(pos)
        .show(ui.ctx(), |ui| {
            let f = Frame::popup(ui.style()).show(ui, |ui| content(ui));
            (f.inner, f.response.rect)
        });

    let (value, frame_rect) = inner.inner;
    st.rect = frame_rect;
    ctx.data_mut(|d| d.insert_temp(state_id, st));
    Some(value)
}

/// Is the anchored popup registered under `id` currently open? Escape
/// handling elsewhere (the mon editor window) must not close while one of
/// these is up: they are `Area` popups with private state, invisible to
/// egui's `any_popup_open()`.
pub fn is_open(ctx: &Context, id: &str) -> bool {
    ctx.data(|d| d.get_temp::<State>(Id::new(id)))
        .map_or(false, |st| st.open)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, Pos2, RawInput};

    /// This module exists for one claim: a popup you can actually type in. A
    /// `ComboBox` popup closes when its own text field is pressed - the bug
    /// reported against every dropdown in the editor - so every test here is about
    /// what a press does to the popup's open state.
    struct H {
        ctx: egui::Context,
        warmed: bool,
        /// Where the trigger button actually was last frame (recorded by the
        /// harness, never guessed from layout assumptions).
        trigger: Rect,
    }

    fn move_to(p: Pos2) -> Vec<Event> {
        vec![Event::PointerMoved(p)]
    }

    fn press(p: Pos2) -> Vec<Event> {
        vec![
            Event::PointerMoved(p),
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
        ]
    }

    fn release(p: Pos2) -> Vec<Event> {
        vec![
            Event::PointerMoved(p),
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            },
        ]
    }

    impl H {
        fn new() -> Self {
            H {
                ctx: egui::Context::default(),
                warmed: false,
                trigger: Rect::NOTHING,
            }
        }

        /// One frame: a trigger button in a fixed Area, and the anchored popup
        /// whose content is a button plus a text field. Returns the content's
        /// value for frames where the popup was open.
        fn frame(&mut self, events: Vec<Event>) -> Option<u32> {
            if !self.warmed {
                self.warmed = true;
                self.frame(vec![]);
            }
            let input = RawInput {
                screen_rect: Some(Rect::from_min_max(Pos2::ZERO, Pos2::new(1000.0, 800.0))),
                focused: true,
                events,
                ..Default::default()
            };
            let cell = std::cell::Cell::new(None);
            let trig = std::cell::Cell::new(Rect::NOTHING);
            let _ = self.ctx.run(input, |ctx| {
                Area::new(Id::new("root"))
                    .fixed_pos(Pos2::new(120.0, 120.0))
                    .show(ctx, |ui| {
                        let btn = ui.button("trigger");
                        trig.set(btn.rect);
                        let out = anchored(ui, "pop", &btn, btn.clicked(), |ui| {
                            let _ = ui.button("row one");
                            let mut buf = String::new();
                            ui.add(egui::TextEdit::singleline(&mut buf).hint_text("value"));
                            7u32
                        });
                        cell.set(out);
                    });
            });
            self.trigger = trig.get();
            cell.into_inner()
        }

        fn state(&self) -> State {
            self.ctx
                .data(|d| d.get_temp::<State>(Id::new("pop")))
                .expect("popup state is stored under its id")
        }

        fn trigger_center(&self) -> Pos2 {
            self.trigger.center()
        }
    }

    /// egui reports `clicked()` on the frame of the *release*, so a click is
    /// move / press / release and the popup appears on the third frame.
    impl H {
        /// Lay the trigger out once, then return where it really is.
        fn trigger(&mut self) -> Pos2 {
            self.frame(vec![]);
            assert!(self.trigger.is_positive(), "the trigger was laid out");
            self.trigger_center()
        }

        /// Warm up, find the trigger, click it.
        fn open(&mut self) {
            let p = self.trigger();
            self.click_at(p);
        }

        fn click_at(&mut self, p: Pos2) -> Vec<Option<u32>> {
            let a = self.frame(move_to(p));
            let b = self.frame(press(p));
            let c = self.frame(release(p));
            vec![a, b, c]
        }
    }

    #[test]
    fn closed_until_the_trigger_is_clicked() {
        let mut h = H::new();
        assert_eq!(h.frame(vec![]), None, "nothing drawn, nothing open");
        let p = h.trigger();
        let out = h.click_at(p);
        assert_eq!(out[2], Some(7), "the release frame opens the popup");
        assert!(h.state().open);
        assert_eq!(h.frame(vec![]), Some(7), "and it stays open");
    }

    #[test]
    fn a_press_on_its_own_text_field_does_not_close_it() {
        // This is the reported bug: in a ComboBox the popup's own field counts as
        // "clicked elsewhere" and the menu vanishes the moment you aim for it.
        let mut h = H::new();
        h.open();
        let popup = h.state().rect;
        assert!(popup.is_positive(), "the popup recorded its own rect");

        // Press in the middle of the popup: that is where its text field sits.
        let at = popup.center();
        h.frame(move_to(at));
        h.frame(press(at));
        assert_eq!(
            h.frame(release(at)),
            Some(7),
            "a click inside keeps it open"
        );
        assert!(h.state().open, "and the state stays open");
    }

    #[test]
    fn a_press_elsewhere_closes_it() {
        let mut h = H::new();
        h.open();
        assert!(h.state().open);
        let far = Pos2::new(700.0, 700.0);
        h.frame(move_to(far));
        h.frame(press(far));
        assert_eq!(h.frame(release(far)), None, "pressing outside dismisses it");
        assert!(!h.state().open);
    }

    #[test]
    fn escape_closes_it() {
        let mut h = H::new();
        h.open();
        assert!(h.state().open);
        h.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }]);
        assert!(!h.state().open, "Escape must dismiss the popup");
    }
}
