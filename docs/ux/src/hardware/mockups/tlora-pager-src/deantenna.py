"""Remove the antennas from the product photo: crop above the body and
redraw the top edge and corners where the antenna bases covered them.

    python3 deantenna.py ../../../../../../site/static/images/boards/tlora-pager.png pager-photo.png

The outline constants are measured from that photo and fit no other.
"""
import math
import sys
from PIL import Image

src, out = sys.argv[1], sys.argv[2]
im = Image.open(src).convert("RGBA")
px = im.load()
w, h = im.size
L, R, TOP, RAD = 13.0, 314.5, 159.3, 30.0  # body outline, measured
# Clean columns of the straight top edge just inside each patch, so the
# patch takes the local shading.
refs = {"left": [px[66, y] for y in range(h)],
        "right": [px[266, y] for y in range(h)]}


def depth(x, y):
    """Signed distance inside the body's rounded top outline."""
    if x < L + RAD:
        cx = L + RAD
    elif x > R - RAD:
        cx = R - RAD
    else:
        return y - TOP
    cy = TOP + RAD
    if y > cy:
        return min(x - L, R - x)
    return RAD - math.hypot(x - cx, y - cy)


def shade(x, y, ref):
    if L + RAD <= x <= R - RAD - 1:
        return ref[y]  # straight edge: the photo's own profile
    s = 4
    cover = sum(depth(x + (i + .5) / s, y + (j + .5) / s) > 0
                for i in range(s) for j in range(s)) / (s * s)
    d = max(0.0, depth(x + .5, y + .5))
    r, g, b, _ = ref[min(161 + int(d), 172)]
    return (r, g, b, round(255 * cover))


for y in range(0, int(TOP) - 1):
    for x in range(w):
        px[x, y] = (0, 0, 0, 0)
# Where the antenna bases sat, and the haze around them.
FEATHER = 6
for y in range(int(TOP) - 1, 173):
    for x, side, inner in ([(x, "left", 63) for x in range(10, 64)]
                           + [(x, "right", 268) for x in range(268, 320)]):
        new = shade(x, y, refs[side])
        t = 1 - abs(x - inner) / FEATHER  # blend toward the photo at the seam
        if t > 0 and px[x, y][3] > 200 and new[3] > 200:
            old = px[x, y]
            new = tuple(round(n * (1 - t) + o * t) for n, o in zip(new, old))
        px[x, y] = new
im.crop((0, int(TOP) - 2, w, h)).save(out)
