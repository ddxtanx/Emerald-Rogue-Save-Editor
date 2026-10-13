#!/usr/bin/env python3
"""Fix the picker trigger's icon placement.

The trigger button reserves `icon_pad` px on its left for the sprite, but the
sprite was painted at `rect.left_center() - vec2(icon_pad*0.5 + 4, ...)`, i.e.
starting 20px to the LEFT of the button. At a panel edge the panel border sliced
it, which is what reads as "the pokemon next to the search bar is cut off
horizontally". Also gives the icon rect its own function so containment is
testable.
"""
import pathlib

p = pathlib.Path("src/searchable_combo.rs")
s = p.read_text()

# 1. undo the width theory (the popup is already as wide as its Area)
old_width = """    if open {
        // The popup must cover the whole line the button sits on, not just the
        // button: a picker as narrow as its label floats over a much wider roster
        // row, and the row sticking out on both sides of it reads as the pokemon
        // being cut off. `anchor_width` is what the button had to work with.
        let popup_width = (opts
            .min_width
            .max(anchor_width)
            .max(btn_response.rect.width()))
        .min(ctx.screen_rect().width() - 16.0);
"""
assert old_width in s
s = s.replace(old_width, "    if open {\n")

s = s.replace(
    """            min_width: popup_width,
            max_height: opts.max_height,
        }""",
    """            min_width: opts.min_width.max(btn_response.rect.width()),
            max_height: opts.max_height,
        }""",
)

old_anchor = """    // Width available to the trigger at its allocation point: the popup anchors to
    // this, so it covers the row behind it instead of floating narrower than it.
    let anchor_width = ui.available_width();

"""
assert old_anchor in s
s = s.replace(old_anchor, "")

# 2. the actual fix: sprite inside the reservation
old_icon = """            if icon_pad > 0.0 {
                let icon_rect = trigger_icon_rect(rect, icon_pad);
                icons::paint_icon("""
assert old_icon in s
helper = """/// Where a trigger button paints its sprite. The button reserves `icon_pad` px on
/// its left; the sprite has to sit **inside** that reservation. It used to be
/// painted at `left_center() - (icon_pad/2 + 4)`, which starts it ~20px left of the
/// button itself - at a panel edge the border sliced it in half, which is exactly
/// the "the pokemon next to the search bar is cut off horizontally" report.
fn trigger_icon_rect(rect: Rect, icon_pad: f32) -> Rect {
    let side = ICON_SIZE.min(rect.height()).min(icon_pad);
    Rect::from_min_size(
        rect.left_top() + egui::vec2(4.0, (rect.height() - side) * 0.5),
        Vec2::splat(side),
    )
}

"""
s = s.replace(
    "const ICON_SIZE: f32 = 24.0;", "const ICON_SIZE: f32 = 24.0;\n\n" + helper, 1
)

p.write_text(s)

# 3. replace the weak width test with the containment test
t = pathlib.Path("src/searchable_combo/tests.rs")
ts = t.read_text()
start = ts.index("/// The popup must be as wide as the line")
end = ts.index("/// The picker's own button", start)
ts = (
    ts[:start]
    + """/// A picker at a panel's left edge had its sprite sliced by that edge: the sprite
/// was painted left of the button instead of inside the gap reserved for it.
#[test]
fn trigger_icon_stays_inside_the_button() {
    let rect = Rect::from_min_size(Pos2::new(8.0, 100.0), Vec2::new(320.0, 24.0));
    let icon = trigger_icon_rect(rect, ICON_SIZE + 8.0);
    assert!(
        rect.contains(icon.min) && rect.contains(icon.max),
        "icon {icon:?} escapes the trigger {rect:?}; a sprite painted outside the \\
         button is sliced by whatever edge the button sits against"
    );
}

"""
    + ts[end:]
)
t.write_text(ts)
print("patched")
