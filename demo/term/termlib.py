"""Shared bits for the terminal feature demos: raw mode, output, replies."""
import fcntl
import os
import select
import struct
import sys
import termios
import tty

OUT = sys.stdout.buffer
ESC = "\x1b"
CSI = "\x1b["
OSC = "\x1b]"
ST = "\x1b\\"
APC = "\x1b_"
DCS = "\x1bP"


def write(*parts):
    for part in parts:
        OUT.write(part if isinstance(part, bytes) else part.encode())
    OUT.flush()


def size():
    try:
        rows, cols, xpx, ypx = struct.unpack(
            "HHHH", fcntl.ioctl(1, termios.TIOCGWINSZ, b"\0" * 8))
        return cols, rows, xpx, ypx
    except OSError:
        return 80, 24, 0, 0


class Raw:
    """Raw mode on stdin for the `with` block."""

    def __init__(self, fd=0):
        self.fd = fd
        self.saved = None

    def __enter__(self):
        if os.isatty(self.fd):
            self.saved = termios.tcgetattr(self.fd)
            tty.setraw(self.fd)
        return self

    def __exit__(self, *exc):
        if self.saved is not None:
            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.saved)


def read_available(timeout=0.3):
    """Everything the terminal sent within `timeout` seconds."""
    out = b""
    deadline = timeout
    while True:
        ready, _, _ = select.select([0], [], [], deadline)
        if not ready:
            return out
        chunk = os.read(0, 4096)
        if not chunk:
            # Stdin closed: nobody is driving the demo.
            raise SystemExit(0)
        out += chunk
        deadline = 0.05


def wait_key(prompt="\r\n[any key: next, q: quit] "):
    write(CSI + "0m", prompt)
    while True:
        ready, _, _ = select.select([0], [], [], None)
        if ready:
            data = os.read(0, 64)
            if data in (b"", b"q", b"Q", b"\x03"):
                return None
            return data


def show(data):
    """Escape sequences made visible."""
    out = []
    for b in data:
        if b == 0x1b:
            out.append("ESC")
        elif b < 0x20 or b == 0x7f:
            out.append("^%c" % (b ^ 0x40))
        elif b < 0x80:
            out.append(chr(b))
        else:
            out.append("\\x%02x" % b)
    return "".join(out)


def clear():
    write(CSI + "2J" + CSI + "H")


def title(text):
    write(CSI + "1;4m", text, CSI + "0m\r\n\r\n")
