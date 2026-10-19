"""Interactive modes: mouse, focus, paste, kitty keyboard, cursor, colors,
title, clipboard, notifications, in-band resize, synchronized output."""
import os
import re
import time

from termlib import *

HELP = [
    "F1 flash (DECSCNM ?5)      F2 toggle pixel mouse (1016)   F3 cycle cursor shape",
    "F4 OSC 10/11 colors        F5 reset colors (110/111)      F6 bell",
    "F7 OSC 52 copy             F8 notifications (OSC 9/777)   F9 cycle title",
    "F10 toggle alt-scroll 1007 F11 toggle sync wrapping       q quit",
    "Type, click, drag, wheel, paste, focus in/out, resize the window.",
]
events = []
state = {"pixel": False, "cursor": 0, "colors": False, "title": 0, "altscroll": True, "sync": True, "frames": 0}


def log(kind, text):
    events.append((kind, text))
    del events[:-14]


def decode(data):
    """Splits input into sequences and labels the known ones."""
    i = 0
    while i < len(data):
        if data[i:i + 1] == b"\x1b" and data[i + 1:i + 2] == b"[":
            m = re.match(rb"\x1b\[([<>?=]?)([\d;:]*)([ -/]*)([@-~])", data[i:])
            if m:
                prefix, params, inter, final = (m.group(1).decode(), m.group(2).decode(), m.group(3).decode(), m.group(4).decode())
                i += m.end()
                yield label(prefix, params, final, m.group(0))
                continue
        if data[i:i + 1] == b"\x1b" and data[i + 1:i + 2] == b"]":
            end = data.find(b"\x1b\\", i)
            end = len(data) if end < 0 else end + 2
            yield ("osc", show(data[i:end]))
            i = end
            continue
        yield ("text", show(data[i:i + 1]))
        i += 1


def label(prefix, params, final, raw):
    p = params.split(";") if params else []
    if prefix == "<" and final in "Mm":
        b, x, y = (int(v) for v in p[:3])
        button = b & 3
        mods = "+".join(n for bit, n in ((4, "shift"), (8, "alt"), (16, "ctrl")) if b & bit)
        kind = "wheel %s" % ("up" if button == 0 else "down") if b & 64 else (
            "motion" if b & 32 else ("press" if final == "M" else "release"))
        unit = "px" if state["pixel"] else "cell"
        return ("mouse", "%s button %d at %d,%d %s %s" % (kind, button, x, y, unit, mods))
    if final == "u" and prefix == "":
        key = p[0].split(":") if p else [""]
        mods = p[1].split(":") if len(p) > 1 else ["1"]
        text = p[2] if len(p) > 2 else ""
        cp = int(key[0] or 0)
        name = chr(cp) if 32 <= cp < 0x110000 and cp not in (0x7f,) else "cp%d" % cp
        shifted = (" shifted=%s" % chr(int(key[1]))) if len(key) > 1 and key[1] else ""
        base = (" base=%s" % chr(int(key[2]))) if len(key) > 2 and key[2] else ""
        kinds = {"1": "press", "2": "repeat", "3": "release"}
        kind = kinds.get(mods[1], "press") if len(mods) > 1 else "press"
        m = int(mods[0]) - 1
        modnames = "+".join(n for bit, n in ((1, "shift"), (2, "alt"), (4, "ctrl"), (8, "super"), (64, "caps"), (128, "num")) if m & bit)
        return ("key", "%s %s%s%s %s%s" % (kind, name, shifted, base, modnames, (" text=" + "".join(chr(int(c)) for c in text.split(":") if c)) if text else ""))
    if final in "PQRS" and prefix == "" and (params == "" or params.startswith("1")):
        # F1-F4 as CSI [1;mods] P/Q/R/S (kitty's form for them).
        return ("key", "function key (legacy) F%d%s" % ("PQRS".index(final) + 1,
                (" mods=" + params.split(";")[1]) if ";" in params else ""))
    if final == "I":
        return ("focus", "focus in")
    if final == "O":
        return ("focus", "focus out")
    if final == "~" and params in ("200", "201"):
        return ("paste", "paste %s" % ("begin" if params == "200" else "end"))
    if final == "t" and p and p[0] == "48":
        return ("resize", "in-band resize: %s rows x %s cols, %s x %s px" % tuple(p[1:5]))
    if final == "t":
        return ("report", "window report " + params)
    if final == "~" and params.split(";")[0] in ("11", "12", "13", "14", "15", "17", "18", "19", "20", "21", "23", "24"):
        return ("key", "function key (legacy) " + show(raw))
    return ("seq", show(raw))


def draw():
    cols, rows, xpx, ypx = size()
    out = [CSI + "?2026h" if state["sync"] else "", CSI + "H"]
    out.append(CSI + "1;44;97m" + (" dekit modes demo  %dx%d cells, %dx%d px, frame %d " % (cols, rows, xpx, ypx, state["frames"])).ljust(cols) + CSI + "0m\r\n")
    for line in HELP:
        out.append(CSI + "2K" + CSI + "36m" + line[:cols] + CSI + "0m\r\n")
    out.append(CSI + "2K" + ("pixel mouse: %s  cursor: %d  colors: %s  alt-scroll: %s  sync: %s" % (
        state["pixel"], state["cursor"], state["colors"], state["altscroll"], state["sync"]))[:cols] + "\r\n")
    out.append(CSI + "2K" + "-" * cols + "\r\n")
    for kind, text in events:
        color = {"mouse": "33", "key": "32", "focus": "35", "paste": "34", "resize": "31", "osc": "36"}.get(kind, "0")
        out.append(CSI + "2K" + CSI + color + "m" + ("%-7s" % kind) + CSI + "0m " + text[:cols - 9] + "\r\n")
    for _ in range(rows - 7 - len(events) - 2):
        out.append(CSI + "2K\r\n")
    out.append(CSI + "2K" + "Type here (text echoes with its kitty encoding when the protocol is on): " + "".join(t for k, t in events if k == "text")[-20:])
    if state["sync"]:
        out.append(CSI + "?2026l")
    write("".join(out))


TITLES = ["dekit modes", "title two", "测试 title"]

with Raw():
    write(CSI + "?1049h" + CSI + "?1003h" + CSI + "?1006h" + CSI + "?1004h" + CSI + "?2004h" + CSI + "?2048h" + CSI + ">15u")
    write(OSC + "0;dekit modes" + ST)
    try:
        while True:
            state["frames"] += 1
            draw()
            data = read_available(1.0)
            if not data:
                continue
            for kind, text in decode(data):
                log(kind, text)
                if kind == "text" and text in ("q", "^C"):
                    raise KeyboardInterrupt
                if kind == "key" and " q " in text + " " and "press" in text:
                    raise KeyboardInterrupt
                m = re.search(r"cp(\d+)|function key \(legacy\) ESC\[(\d+)~|ESC O(\w)", text)
                fkey = None
                if "cp" in text and kind == "key":
                    cp = int(re.search(r"cp(\d+)", text).group(1))
                    fkey = {57364: 1, 57365: 2, 57366: 3, 57367: 4, 57368: 5, 57369: 6, 57370: 7, 57371: 8, 57372: 9, 57373: 10, 57374: 11}.get(cp)
                    if "release" in text:
                        fkey = None
                elif kind == "key" and "legacy" in text:
                    m = re.search(r"ESC\[(\d+)~", text)
                    if m:
                        n = int(m.group(1))
                        fkey = {11: 1, 12: 2, 13: 3, 14: 4, 15: 5, 17: 6, 18: 7, 19: 8, 20: 9, 21: 10, 23: 11}.get(n)
                    else:
                        fkey = int(re.search(r"F(\d+)", text).group(1))
                elif kind == "seq" and re.match(r"ESC O[PQRS]", text):
                    fkey = "PQRS".index(text[-1]) + 1
                if fkey == 1:
                    write(CSI + "?5h")
                    time.sleep(0.15)
                    write(CSI + "?5l")
                elif fkey == 2:
                    state["pixel"] = not state["pixel"]
                    write(CSI + ("?1016h" if state["pixel"] else "?1016l" + CSI + "?1006h"))
                elif fkey == 3:
                    state["cursor"] = (state["cursor"] + 1) % 7
                    write(CSI + "%d q" % state["cursor"])
                elif fkey == 4:
                    state["colors"] = True
                    write(OSC + "11;rgb:20/10/40" + ST + OSC + "10;#ffe0a0" + ST)
                elif fkey == 5:
                    state["colors"] = False
                    write(OSC + "111" + ST + OSC + "110" + ST)
                elif fkey == 6:
                    write("\x07")
                elif fkey == 7:
                    import base64
                    write(OSC + "52;c;" + base64.b64encode(b"hello from the dekit modes demo").decode() + ST)
                    log("osc", "copied 'hello from the dekit modes demo' to the clipboard")
                elif fkey == 8:
                    write(OSC + "9;OSC 9 notification body" + ST)
                    write(OSC + "777;notify;dekit;OSC 777 notification" + ST)
                elif fkey == 9:
                    state["title"] = (state["title"] + 1) % len(TITLES)
                    write(OSC + "2;" + TITLES[state["title"]] + ST)
                elif fkey == 10:
                    state["altscroll"] = not state["altscroll"]
                    write(CSI + ("?1007h" if state["altscroll"] else "?1007l"))
                elif fkey == 11:
                    state["sync"] = not state["sync"]
    except KeyboardInterrupt:
        pass
    finally:
        write(CSI + "<u" + CSI + "?2048l" + CSI + "?2004l" + CSI + "?1004l" + CSI + "?1006l" + CSI + "?1016l" + CSI + "?1003l" + CSI + "0 q" + OSC + "111" + ST + OSC + "110" + ST + CSI + "?1049l")
