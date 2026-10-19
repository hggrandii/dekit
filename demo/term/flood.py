"""Throughput: 200k styled lines as fast as the pty takes them, timed."""
import sys
import time

from termlib import *

n = int(sys.argv[1]) if len(sys.argv) > 1 else 200_000
start = time.time()
chunk = []
for i in range(n):
    chunk.append("%s%07d%s %s%s%s %s\r\n" % (
        CSI + "2m", i, CSI + "0m",
        CSI + "3%dm" % (1 + i % 6), ["ok", "warn", "ERR ", "info", "测试", "\U0001F600"][i % 6], CSI + "0m",
        "payload " * (1 + i % 5)))
    if len(chunk) >= 2000:
        write("".join(chunk))
        chunk = []
write("".join(chunk))
elapsed = time.time() - start
write(CSI + "1m" + "%d lines in %.2fs (%.0f lines/s). Scroll back; the last line is numbered %07d." % (n, elapsed, n / max(elapsed, 1e-9), n - 1) + CSI + "0m\r\n")
