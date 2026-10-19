"""SGR gallery: colors, attributes, underline styles and colors, links."""
from termlib import *

clear()
title("SGR: attributes")
for code, name in [(1, "bold"), (2, "dim"), (3, "italic"), (4, "underline"),
                   (5, "blink"), (7, "inverse"), (8, "hidden (shown as blank)"),
                   (9, "strike"), (53, "overline")]:
    write(CSI + "%dm" % code, "%-26s" % name, CSI + "0m", "  plain\r\n")
write(CSI + "1;3;4;9m", "bold italic underline strike", CSI + "0m\r\n")
write(CSI + "1m", "bold then ", CSI + "22m", "22 ends bold+dim ", CSI + "2m", "dim", CSI + "0m\r\n\r\n")

title("SGR 4:n underline styles (fall back to plain underline)")
for n, name in [(1, "single"), (2, "double"), (3, "curly"), (4, "dotted"), (5, "dashed")]:
    write(CSI + "4:%dm" % n, "4:%d %s" % (n, name), CSI + "0m", "   ")
write("\r\n")
write(CSI + "4:3m" + CSI + "58:2::255:80:80m", "curly red underline", CSI + "0m  ")
write(CSI + "4:1m" + CSI + "58:5:33m", "indexed 33 underline", CSI + "59;24m", "\r\n\r\n")

title("16 colors (30-37, 90-97; backgrounds 40-47, 100-107)")
for base in (30, 90):
    for i in range(8):
        write(CSI + "%dm" % (base + i), " %2d " % (base + i))
    write(CSI + "0m  ")
    for i in range(8):
        write(CSI + "%dm" % (base + 10 + i), "    ")
    write(CSI + "0m\r\n")
write("\r\n")

title("256 colors (38;5;n)")
for row in range(16, 232, 36):
    for c in range(row, row + 36):
        write(CSI + "48;5;%dm" % c, " ")
    write(CSI + "0m\r\n")
for c in range(232, 256):
    write(CSI + "48;5;%dm" % c, "  ")
write(CSI + "0m\r\n\r\n")

title("True color (38;2;r;g;b) and colon form (38:2::r:g:b)")
cols = size()[0] - 2
for y in range(4):
    for x in range(cols):
        r = int(255 * x / max(cols - 1, 1))
        g = int(255 * (1 - x / max(cols - 1, 1)))
        b = 60 * y
        write(CSI + "48;2;%d;%d;%dm" % (r, g, b), " ")
    write(CSI + "0m\r\n")
write(CSI + "38:2::255:200:0m", "colon-form yellow", CSI + "0m ",
      CSI + "38;2;0;200;255m", "semicolon cyan", CSI + "0m\r\n\r\n")

title("OSC 8 hyperlinks (hover/click in a terminal that supports them)")
write(OSC + "8;;https://github.com/pvolok/dekit" + ST, "dekit on GitHub", OSC + "8;;" + ST, "  ")
write(OSC + "8;id=same;https://example.com" + ST, "same-id ", OSC + "8;;" + ST, "gap ",
      OSC + "8;id=same;https://example.com" + ST, "link", OSC + "8;;" + ST, "\r\n")
write(CSI + "1;34m" + OSC + "8;;https://example.org" + ST, "styled link", OSC + "8;;" + ST + CSI + "0m\r\n\r\n")

title("SGR stack (XTPUSHSGR / XTPOPSGR) and reset forms")
write(CSI + "1;31m", "red bold ", CSI + "#{", CSI + "32m", "green pushed ", CSI + "#}", "popped red bold", CSI + "m", "\r\n")
write("Done. Scroll back with the wheel or copy mode to check the top.\r\n")
with Raw():
    wait_key("\r\n[q to quit] ")
