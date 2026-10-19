"""A sixel color wheel, with the DA1 check a sixel program would do."""
import colorsys

from termlib import *


def sixel(w, h, pixel):
    """`pixel(x, y) -> palette index` with a 64-color palette."""
    out = [DCS + "0;0;0q" + '"1;1;%d;%d' % (w, h)]
    for i in range(64):
        r, g, b = colorsys.hsv_to_rgb((i % 16) / 16, 1.0, 0.35 + 0.65 * (i // 16) / 3)
        out.append("#%d;2;%d;%d;%d" % (i, r * 100, g * 100, b * 100))
    for band in range(0, h, 6):
        for color in range(64):
            row = []
            used = False
            for x in range(w):
                bits = 0
                for dy in range(6):
                    y = band + dy
                    if y < h and pixel(x, y) == color:
                        bits |= 1 << dy
                used |= bits != 0
                row.append(chr(63 + bits))
            if used:
                out.append("#%d" % color + "".join(row) + "$")
        out.append("-")
    out.append(ST)
    return "".join(out)


clear()
title("Sixel (DCS q): a 96x72 color wheel, then DA1")
w, h = 96, 72


def wheel(x, y):
    import math
    dx, dy = x - w / 2, y - h / 2
    d = (dx * dx + dy * dy) ** .5
    if d > min(w, h) / 2:
        return 63
    hue = (math.atan2(dy, dx) / (2 * math.pi)) % 1.0
    ring = min(3, int(4 * d / (min(w, h) / 2)))
    return int(hue * 16) % 16 + 16 * ring


write(sixel(w, h, wheel))
write("\r\n<- a sixel image (decoded by dekit, shown as a kitty placement)\r\n\r\n")
with Raw():
    write(CSI + "c")
    data = read_available(0.5)
    write("DA1 reply: ", show(data), "\r\n")
    write("No parameter 4: dekit decodes sixel but does not advertise it, so\r\n"
          "programs with a choice (yazi, chafa) use kitty graphics instead.\r\n")
    wait_key("\r\n[q to quit] ")
