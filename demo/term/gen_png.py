"""Writes a PNG for programs that show image files (kitty icat)."""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from imglib import png, rgba_disc  # noqa: E402

path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/dekit-sample.png"
w, h = 256, 160
disc = bytearray(rgba_disc(w, h, (255, 140, 0)))
# A gradient background behind the disc.
for y in range(h):
    for x in range(w):
        i = (y * w + x) * 4
        if disc[i + 3] == 0:
            disc[i:i + 4] = bytes((x % 256, y % 256, 180, 255))
with open(path, "wb") as f:
    f.write(png(w, h, bytes(disc)))
print(path)
