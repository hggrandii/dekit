"""Every query a program might send, with the terminal's answer."""
from termlib import *

QUERIES = [
    ("DA1  CSI c", CSI + "c"),
    ("DA2  CSI > c", CSI + ">c"),
    ("DA3  CSI = c", CSI + "=c"),
    ("XTVERSION  CSI > 0 q", CSI + ">0q"),
    ("DSR 5 (status)", CSI + "5n"),
    ("CPR  CSI 6 n", CSI + "6n"),
    ("DECXCPR  CSI ? 6 n", CSI + "?6n"),
    ("DECRQM ?2026 (sync)", CSI + "?2026$p"),
    ("DECRQM ?2027 (graphemes)", CSI + "?2027$p"),
    ("DECRQM ?2004 (paste)", CSI + "?2004$p"),
    ("DECRQM ?1049 (alt)", CSI + "?1049$p"),
    ("DECRQM ?7 (autowrap)", CSI + "?7$p"),
    ("DECRQM ?1007 (alt scroll)", CSI + "?1007$p"),
    ("DECRQM ?69 (margins)", CSI + "?69$p"),
    ("DECRQM 4 (insert)", CSI + "4$p"),
    ("DECRQM ?9999 (unknown)", CSI + "?9999$p"),
    ("XTWINOPS 14t text px", CSI + "14t"),
    ("XTWINOPS 16t cell px", CSI + "16t"),
    ("XTWINOPS 18t cells", CSI + "18t"),
    ("DECRQSS DECSTBM", DCS + "$qr" + ST),
    ("DECRQSS SGR", DCS + "$qm" + ST),
    ("DECRQSS DECSCUSR", DCS + "$q q" + ST),
    ("XTGETTCAP TN RGB colors", DCS + "+q" + "544e;524742;636f6c6f7273" + ST),
    ("XTGETTCAP smxx (strike)", DCS + "+q" + "736d7878" + ST),
    ("XTGETTCAP nope", DCS + "+q" + "6e6f7065" + ST),
    ("kitty keyboard  CSI ? u", CSI + "?u"),
    ("OSC 10 ;? fg", OSC + "10;?" + ST),
    ("OSC 11 ;? bg", OSC + "11;?" + ST),
    ("OSC 12 ;? cursor", OSC + "12;?" + ST),
    ("OSC 4 ;1;? palette red", OSC + "4;1;?" + ST),
    ("graphics query", APC + "Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA" + ST),
    ("XTSMGRAPHICS colors", CSI + "?1;1;0S"),
    ("XTSMGRAPHICS sixel geometry", CSI + "?2;1;0S"),
]

clear()
title("Queries and the replies dekit gives (a reply arrives within 300 ms)")
with Raw():
    write(CSI + "1;31m" + CSI + "4:3m")  # a style, so DECRQSS SGR has something to say
    for name, query in QUERIES:
        write(query)
        data = read_available(0.3)
        write(CSI + "0m" + CSI + "36m", "%-30s" % name, CSI + "0m", show(data) or CSI + "2m(no reply)" + CSI + "0m", "\r\n")
    write(CSI + "0m\r\nAfter pushing kitty flags 31 and querying again:\r\n")
    write(CSI + ">31u" + CSI + "?u")
    write("%-30s" % "kitty keyboard", show(read_available(0.3)), "\r\n")
    write(CSI + "<u")
    write("\r\nSet margins and a scroll region, then DECRQSS:\r\n")
    write(CSI + "?69h" + CSI + "5;60s" + CSI + "3;20r")
    write(DCS + "$qs" + ST)
    write("%-30s" % "DECRQSS DECSLRM", show(read_available(0.3)), "\r\n")
    write(DCS + "$qr" + ST)
    write("%-30s" % "DECRQSS DECSTBM", show(read_available(0.3)), "\r\n")
    write(CSI + "r" + CSI + "?69l")
    wait_key("\r\n[q to quit] ")
