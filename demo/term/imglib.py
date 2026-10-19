"""Image builders and the kitty graphics command writer, shared by the demos."""
import base64
import struct
import zlib

from termlib import *


def rgb_gradient(w, h, phase=0):
    out = bytearray()
    for y in range(h):
        for x in range(w):
            out += bytes((int(255 * x / max(w - 1, 1)), int(255 * y / max(h - 1, 1)), (phase * 60) % 256))
    return bytes(out)


def rgba_disc(w, h, color=(255, 120, 0)):
    out = bytearray()
    cx, cy, r = w / 2, h / 2, min(w, h) / 2
    for y in range(h):
        for x in range(w):
            d = ((x + .5 - cx) ** 2 + (y + .5 - cy) ** 2) ** .5
            a = 255 if d < r - 1 else (int(255 * (r - d)) if d < r else 0)
            out += bytes(color) + bytes((max(0, a),))
    return bytes(out)


def png(w, h, rgba):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    raw = b"".join(b"\0" + rgba[y * w * 4:(y + 1) * w * 4] for y in range(h))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def g(control, payload=b""):
    """One kitty graphics command, chunked at 4096 base64 bytes."""
    data = base64.b64encode(payload)
    if not data:
        write(APC + "G" + control + ST)
        return
    first = True
    while data:
        head, data = data[:4096], data[4096:]
        keys = control if first else "q=" + ("2" if "q=2" in control else "0")
        first = False
        write(APC + "G" + keys + ",m=%d;" % (1 if data else 0), head, ST)


