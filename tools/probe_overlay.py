"""Where exactly does the popup panel sit, and what does it cover?

Measure (no vision, deterministic):
  * the popup frame's left/right edges  = the long vertical colour transitions
  * where the pokemon sprites sit in the roster list *below* the popup
  * where the list rows sit *inside* the popup
If the sprites in the background row start left of the popup's left edge, the
popup visually slices them.
"""
import json
from PIL import Image

im = Image.open("/workspace/hermes/er-save-editor/shot_menu.png").convert("RGB")
W, H = im.size
px = im.load()
bg = px[1, 1]


def diff(c, ref=bg, t=14):
    return abs(c[0] - ref[0]) + abs(c[1] - ref[1]) + abs(c[2] - ref[2]) > t


def col_ink(y0, y1):
    """Columns that have any non-background pixel between y0..y1."""
    return [x for x in range(W) if any(diff(px[x, y]) for y in range(y0, y1, 2))]


print("image", W, H, "bg", bg)

# Row-band profiles: find the popup by looking for a horizontal run that is
# uniformly window-fill across most of the width.
for y in range(0, H, 20):
    row = [x for x in range(0, W, 4) if diff(px[x, y])]
    print(f"y={y:4d} ink_cols={len(row):4d} first={row[0] if row else '-':>4} last={row[-1] if row else '-'}")
