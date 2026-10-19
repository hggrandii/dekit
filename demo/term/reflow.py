"""Resize the pane: long lines rewrap, wide glyphs stay whole, images and
the cursor follow their text; scroll back to see history rewrapped too."""
import base64
import sys

from termlib import *

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from imglib import rgb_gradient, g  # noqa: E402

clear()
title("Reflow: resize the pane now (also while scrolled back)")
for i in range(1, 9):
    write(CSI + "3%dm" % (1 + i % 6), "%02d " % i, CSI + "0m",
          "a long line of text that wraps at the pane width, then rewraps at the new width when the pane is resized; "
          "numbers mark its start " * 2, "\r\n")
write("\r\nwide: 测试测试测试测试测试测试测试测试测试测试测试测试测试测试 (CJK, never split)\r\n\r\n")
write("image anchored after this text: ")
g("a=T,f=24,s=32,v=32,c=8,r=4,i=70,q=2", rgb_gradient(32, 32, 1))
write("\r\n" * 4)
write("more text after the image; the image must keep its place relative to these lines\r\n")
for i in range(9, 14):
    write("%02d " % i, "short line\r\n")
write("The cursor is here -> ")
with Raw():
    wait_key("")
g("a=d,d=A,q=2")
