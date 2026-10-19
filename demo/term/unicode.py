"""Width policy gallery: every row's text should end before the bar."""
from termlib import *

clear()
title("Unicode widths: text must end before the | at column 40")
cols = 40
ruler = "".join(str(i % 10) for i in range(cols))
write(CSI + "2m", "          ", ruler[10:], CSI + "0m\r\n")
samples = [
    ("ascii", "hello world"),
    ("cjk wide", "测试中文 テスト"),
    ("hangul", "한글 테스트"),
    ("emoji", "\U0001F600\U0001F680\U0001F4BB"),
    ("vs16 widen", "❤️ ✔️ ⬆️"),
    ("vs15 narrow", "❤︎ ✔︎"),
    ("flags (RI)", "\U0001F1FA\U0001F1F8 \U0001F1EF\U0001F1F5 \U0001F1E9\U0001F1EA"),
    ("zwj family", "\U0001F468‍\U0001F469‍\U0001F467‍\U0001F466"),
    ("skin tone", "\U0001F44D\U0001F3FD \U0001F9D1\U0001F3FF"),
    ("keycap", "1️⃣ #️⃣"),
    ("combining", "é ä ñ स्क"),
    ("box drawing", "┌─┬┐ │ └┴┘"),
    ("blocks", "█▓▒░ ▄▀"),
    ("math", "∀x∈ℝ ∫∂ ∞"),
    ("arabic", "مرحبا"),
    ("thai", "สวัสดี"),
    ("fullwidth", "Ｈｅｌｌｏ"),
    ("zwj + text", "\U0001F468‍abc (stray ZWJ)"),
    ("tab stops", "a\tb\tc"),
]
for label, text in samples:
    write(CSI + "36m", "%-10s" % label, CSI + "0m", text, CSI + "%dG" % (cols + 1), "|", "\r\n")
write("\r\nMove the cursor over the rows with the mouse in copy mode: cells and\r\n"
      "selection should follow the glyphs, including the right half of wide ones.\r\n")
write("\r\nA grid of wide cells, half overwritten (the other half must blank):\r\n")
write("测试测试测试" + CSI + "2G" + "X" + CSI + "9G" + "Y\r\n")
with Raw():
    wait_key("\r\n[q to quit] ")
