"""Kitty graphics protocol, section by section. Replies are shown."""
import sys

from imglib import *
from termlib import *


def replies(label="replies"):
    data = read_available(0.4)
    if data:
        write("\r\n", CSI + "33m", label + ": ", CSI + "0m", show(data), "\r\n")


def section(name):
    clear()
    title(name)


with Raw():
    section("1. Direct display (a=T): RGB, zlib RGBA, PNG; text continues after")
    g("a=T,f=24,s=40,v=20,i=1,q=1", rgb_gradient(40, 20))
    write(" <- RGB 40x20 at natural size (cells follow the cell size)\r\n\r\n")
    g("a=T,f=32,o=z,s=48,v=48,c=6,r=3,i=2,q=1", zlib.compress(rgba_disc(48, 48)))
    write(" <- RGBA disc, zlib, scaled to 6x3 cells (transparent corners show text)\r\n\r\n")
    g("a=T,f=100,c=10,r=5,i=3,q=1", png(32, 32, rgba_disc(32, 32, (80, 160, 255))))
    write(" <- PNG 32x32 in 10x5 cells\r\n")
    replies()
    if wait_key() is None:
        sys.exit()

    section("2. Transmit once (a=t), place many (a=p): scale, crop, offsets, z-index")
    g("a=t,f=24,s=64,v=32,i=10,q=2", rgb_gradient(64, 32))
    write(CSI + "4;1H")
    g("a=p,i=10,p=1,c=16,r=4,q=2")
    write(CSI + "4;20H")
    g("a=p,i=10,p=2,c=8,r=2,q=2")
    write(CSI + "7;20H")
    g("a=p,i=10,p=3,x=32,y=0,w=32,h=32,c=8,r=4,q=2")
    write(CSI + "4;30H")
    g("a=p,i=10,p=4,c=8,r=4,X=5,Y=8,q=2")
    write(CSI + "4;40H", "z=-1: image behind this text ->", CSI + "4;72H")
    write(CSI + "4;40H")
    g("a=p,i=10,p=5,c=30,r=4,z=-1,q=2")
    write(CSI + "9;1H", "p1 full 16x4 | p2 8x2 | p3 right half cropped | p4 pixel offsets X=5 Y=8 | p5 behind text (z=-1)")
    replies()
    write(CSI + "12;1H")
    if wait_key() is None:
        sys.exit()

    section("3. Scrolling: placements move with their rows, and come back from history")
    g("a=T,f=24,s=32,v=32,c=8,r=4,i=20,q=2", rgb_gradient(32, 32, 2))
    write(" <- this image is anchored here\r\n")
    for i in range(size()[1] + 4):
        write("line %d scrolls the image up\r\n" % i)
    write("The image is off the top now. Scroll back (wheel / copy mode) to see it.\r\n")
    replies()
    if wait_key() is None:
        sys.exit()

    section("4. Unicode placeholders (U=1): image cells are text, move with it")
    g("a=T,U=1,f=24,s=32,v=32,c=6,r=3,i=30,q=2", rgb_gradient(32, 32, 4))
    marks = ["̅", "̍", "̎", "̐", "̒", "̽"]
    write(CSI + "38;5;30m")
    for r in range(3):
        write(CSI + "%d;5H" % (5 + r))
        for c in range(6):
            write("\U0010eeee" + marks[r] + marks[c])
    write(CSI + "0m")
    write(CSI + "9;1H", "The 6x3 block above is made of placeholder cells (fg = image id 30).\r\n")
    write("Below, the same block drawn by writing the cells one per line and inserting text:\r\n")
    write(CSI + "38;5;30m")
    for r in range(3):
        write("row %d " % r)
        for c in range(6):
            write("\U0010eeee" + marks[r] + marks[c])
        write(CSI + "0m", " <- text after\r\n", CSI + "38;5;30m")
    write(CSI + "0m")
    replies()
    if wait_key() is None:
        sys.exit()

    section("5. Animation: frames (a=f) and a loop (a=a); static viewers show frame 1")
    g("a=T,f=24,s=24,v=24,c=6,r=3,i=40,q=2", rgb_gradient(24, 24, 0))
    for k in range(1, 6):
        g("a=f,i=40,f=24,s=24,v=24,z=150,q=2", rgb_gradient(24, 24, k))
    g("a=a,i=40,s=3,v=1,q=2")
    write(" <- 6 frames, 150ms each, looping (on terminals that animate)\r\n")
    replies()
    if wait_key() is None:
        sys.exit()

    section("6. Deletion variants")
    for i, col in enumerate((1, 12, 23, 34, 45)):
        write(CSI + "4;%dH" % col)
        g("a=T,f=24,s=16,v=16,c=8,r=3,i=%d,p=1,q=2" % (50 + i), rgb_gradient(16, 16, i))
    write(CSI + "8;1H", "five images; deleting: d=i (id 50) ... ")
    g("a=d,d=i,i=50,q=2")
    write("d=c at cursor (image 2) ... ")
    write(CSI + "5;13H")
    g("a=d,d=c,q=2")
    write(CSI + "8;60H", "d=z (none at z=7) ... ")
    g("a=d,d=z,z=7,q=2")
    write(CSI + "9;1H", "Two images (ids 52..54 minus the one at col 23... ) should remain: cols 34 and 45, plus col 23.\r\n")
    write("Then d=A (free all) after the next key.")
    replies()
    if wait_key() is None:
        sys.exit()
    g("a=d,d=A,q=2")
    write("\r\nAll images deleted and freed.\r\n")
    if wait_key() is None:
        sys.exit()

    section("7. Queries and errors (replies below)")
    g("a=q,i=99,f=24,s=1,v=1", b"\0\0\0")
    replies("valid query (expect OK)")
    g("a=q,i=98,f=24,s=2,v=2", b"\0\0\0")
    replies("short data (expect an error)")
    g("a=p,i=777")
    replies("placing an unknown id (expect ENOENT)")
    g("a=t,I=5,f=24,s=2,v=1", rgb_gradient(2, 1))
    replies("transmit by number I=5 (expect i=<id>,I=5;OK)")
    g("a=p,I=5,c=4,r=2")
    replies("place by number")
    write("\r\n")
    if wait_key() is None:
        sys.exit()

    section("8. Large chunked transfer (m=1): 320x160 RGB, zlib")
    g("a=T,f=24,o=z,s=320,v=160,c=40,r=10,i=60,q=2", zlib.compress(rgb_gradient(320, 160, 3)))
    write("\r\n40x10 cells from one 150KB image sent in 4KB chunks.\r\n")
    replies()
    wait_key("\r\n[q or any key: quit] ")
    g("a=d,d=A,q=2")
