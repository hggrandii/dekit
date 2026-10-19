"""A pager in the alternate screen: the wheel scrolls it through alternate
scroll (?1007) as arrow keys; j/k, PgUp/PgDn, g/G, q."""
import re

from termlib import *

lines = ["%4d  %s" % (i, ("lorem ipsum dolor sit amet " * (1 + i % 4)).strip()) for i in range(1, 401)]
top = 0
with Raw():
    write(CSI + "?1049h" + CSI + "?25l")
    try:
        while True:
            cols, rows = size()[:2]
            view = rows - 1
            top = max(0, min(top, len(lines) - view))
            out = [CSI + "H"]
            for i in range(view):
                text = lines[top + i] if top + i < len(lines) else "~"
                out.append(CSI + "2K" + text[:cols] + "\r\n")
            out.append(CSI + "7m" + (" pager  lines %d-%d of %d  wheel/arrows/j/k/PgUp/PgDn/g/G, q quits " % (top + 1, min(top + view, len(lines)), len(lines))).ljust(cols)[:cols] + CSI + "0m")
            write("".join(out))
            data = read_available(None if False else 60)
            if not data:
                continue
            if b"q" in data or b"\x03" in data:
                break
            top += 3 * data.count(b"\x1b[B") + data.count(b"j") + view * (data.count(b"\x1b[6~"))
            top -= 3 * data.count(b"\x1b[A") + data.count(b"k") + view * (data.count(b"\x1b[5~"))
            if b"g" in data:
                top = 0
            if b"G" in data:
                top = len(lines)
    finally:
        write(CSI + "?25h" + CSI + "?1049l")
