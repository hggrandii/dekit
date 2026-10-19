"""Synchronized output: a full-screen repaint 30 times a second; toggle
`?2026` wrapping with s, pause with space, quit with q. Without sync a
slow link shows tearing (half-drawn frames)."""
import time

from termlib import *

state = {"sync": True, "run": True, "frame": 0}
with Raw():
    write(CSI + "?1049h" + CSI + "?25l")
    try:
        while True:
            cols, rows = size()[:2]
            state["frame"] += 1 if state["run"] else 0
            f = state["frame"]
            out = [CSI + "?2026h" if state["sync"] else "", CSI + "H"]
            out.append((" sync %s  frame %d  [s] toggle sync  [space] pause  [q] quit " % ("ON " if state["sync"] else "OFF", f)).ljust(cols)[:cols] + "\r\n")
            for y in range(rows - 1):
                row = []
                for x in range(cols):
                    v = (x + y + f) % 12
                    row.append(CSI + "48;5;%dm" % (232 + v * 2) + (" " if (x + f) % 7 else chr(0x2580 + (x + y) % 16)))
                out.append("".join(row) + CSI + "0m" + ("\r\n" if y < rows - 2 else ""))
            if state["sync"]:
                out.append(CSI + "?2026l")
            write("".join(out))
            data = read_available(1 / 30)
            if b"q" in data or b"\x03" in data:
                break
            if b"s" in data:
                state["sync"] = not state["sync"]
            if b" " in data:
                state["run"] = not state["run"]
    finally:
        write(CSI + "?25h" + CSI + "?1049l")
