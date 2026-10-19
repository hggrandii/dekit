"""Scroll regions, margins, insert/delete, DECALN, REP, tabs, line drawing."""
import time

from termlib import *


def box(x, y, w, h, label):
    """A frame in the DEC line-drawing charset (ESC ( 0)."""
    write(ESC + "(0")
    write(CSI + "%d;%dH" % (y, x) + "l" + "q" * (w - 2) + "k")
    for r in range(1, h - 1):
        write(CSI + "%d;%dH" % (y + r, x) + "x" + CSI + "%dG" % (x + w - 1) + "x")
    write(CSI + "%d;%dH" % (y + h - 1, x) + "m" + "q" * (w - 2) + "j")
    write(ESC + "(B")
    write(CSI + "%d;%dH" % (y, x + 2) + CSI + "1m " + label + " " + CSI + "0m")


def step(seconds=0.08):
    time.sleep(seconds)


with Raw():
    clear()
    title("1. DECSTBM 5-12 with DECSLRM 3-48: a log scrolling inside the frame only")
    box(1, 4, 50, 10, "region rows 5..12, cols 3..48")
    write(CSI + "?69h" + CSI + "3;48s" + CSI + "5;12r" + CSI + "12;3H")
    for i in range(30):
        write(CSI + "12;3H" + "%s%s log line %02d scrolls inside the box only%s\r\n" % (CSI, "3%dm" % (1 + i % 6), i, CSI + "0m"))
        step()
    write(CSI + "r" + CSI + "?69l" + CSI + "16;1H" + "The frame and this text never moved. Scrollback did not receive the region's lines.\r\n")
    if wait_key() is None:
        raise SystemExit

    clear()
    title("2. DECLRMM (?69) + DECSLRM: left/right margins 20-60, scrolling a column")
    box(18, 4, 45, 12, "margins 20..60")
    write(CSI + "?69h" + CSI + "20;60s" + CSI + "5;14r")
    write(CSI + "14;20H")
    for i in range(24):
        write(CSI + "14;20H" + CSI + "9%dm" % (1 + i % 6) + "column text %02d wraps and scrolls between the margins" % i + CSI + "0m" + "\r\n")
        step()
    write(CSI + "r" + CSI + "?69l" + CSI + "17;1H" + "Text outside the margins stayed put; wrapping happened at column 60.\r\n")
    if wait_key() is None:
        raise SystemExit

    clear()
    title("3. IL / DL / ICH / DCH / ECH / DECIC / DECDC")
    for i in range(8):
        write("row %d: the quick brown fox jumps over the lazy dog\r\n" % i)
    write(CSI + "5;1H" + CSI + "2L")
    write(CSI + "5;1H" + CSI + "32m" + "inserted with IL (CSI 2 L)" + CSI + "0m")
    write(CSI + "6;1H" + CSI + "32m" + "second inserted row" + CSI + "0m")
    step(0.6)
    write(CSI + "9;1H" + CSI + "M")
    write(CSI + "13;1H" + CSI + "31m" + "DL (CSI M) removed the row that was at 9" + CSI + "0m")
    step(0.6)
    write(CSI + "3;8H" + CSI + "4@" + CSI + "33m" + "ICH!" + CSI + "0m")
    write(CSI + "4;8H" + CSI + "6P")
    write(CSI + "7;8H" + CSI + "5X")
    write(CSI + "14;1H" + "row 3: ICH 4 then text; row 4: DCH 6; row 7: ECH 5 blanks.")
    step(0.6)
    write(CSI + "?69h" + CSI + "5;40s" + CSI + "2;8r" + CSI + "4;12H" + CSI + "3'}" + CSI + "6;12H" + CSI + "2'~" + CSI + "r" + CSI + "?69l")
    write(CSI + "15;1H" + "DECIC 3 at col 12 (rows 2-8, margins 5-40) and DECDC 2: columns shifted within the margins.\r\n")
    if wait_key() is None:
        raise SystemExit

    clear()
    title("4. REP, tab stops (HTS/TBC), and DECALN")
    write("REP: x" + CSI + "20b" + " <- 'x' repeated 20 more times\r\n")
    write("测" + CSI + "3b" + " <- a wide char repeated (should be 4 wide glyphs)\r\n\r\n")
    write("default tabs:  |" + "\t|" * 6 + "\r\n")
    write(CSI + "3g")
    for col in (5, 9, 16, 30, 45):
        write(CSI + "%dG" % col + ESC + "H")
    write("\r" + CSI + "K" + "custom stops:  " + "\t|" * 5 + "   (5, 9, 16, 30, 45)\r\n")
    write(CSI + "3g" + "cleared stops: a\tb  (tab goes to the end of the line)\r\n")
    write("\r\nDECALN in 2 seconds fills the screen with E (then the next key clears it).")
    step(2.0)
    write(ESC + "#8")
    if wait_key(CSI + "1;1H" + CSI + "0m" + "[any key: next] ") is None:
        raise SystemExit

    clear()
    title("5. Charsets: DEC special graphics via SO/SI (G1) and single shift")
    write(ESC + ")0" + "\x0e" + "lqqqqk\r\nx    x\r\nmqqqqj" + "\x0f" + "   <- drawn with G1 = line drawing, shift out/in\r\n")
    write("single shift SS2: " + ESC + "*0" + ESC + "N" + "a" + " then normal a\r\n")
    write("\r\nAutowrap off (?7l): this very long line should be cut at the right edge instead of wrapping around to the next row like it normally would do\r\n")
    write(CSI + "?7l" + "x" * (size()[0] + 20) + CSI + "?7h" + "\r\n")
    write("Insert mode (IRM 4h): typing shifts text right: " + "ABCDEF" + CSI + "6D" + CSI + "4h" + "123" + CSI + "4l" + "\r\n")
    wait_key("\r\n[q or any key: quit] ")
