# Terminal feature demos

Small programs that exercise one terminal feature family each, for
checking dekit's emulation and client painting by eye. Run them from the
repo root through the tasks in `dekit.yaml` (`term/...`), or directly
with `python3 demo/term/<name>.py`.

| Task | What to look at |
| --- | --- |
| `term/sgr` | Attributes, underline styles and colors, 16/256/true color, OSC 8 links, SGR stack. Compare with the same script in a plain terminal. |
| `term/unicode` | Every row must end before the `\|` at column 40: CJK, emoji, VS16, flags, ZWJ sequences, combining marks. Select text over wide cells in copy mode. |
| `term/images` | Kitty graphics, one section per key press: direct display in three formats, placements (scale, crop, offsets, z-index), scrolling with text and back from history, Unicode placeholders, animation, deletions, queries with replies, chunked transfer. |
| `term/sixel` | A sixel color wheel decoded by dekit, and the DA1 answer. |
| `term/modes` | Mouse (cell and pixel), focus, bracketed paste, kitty keyboard events with modifiers and releases, cursor shapes, reverse video flash, OSC 10/11 colors, bell, OSC 52 clipboard, notifications, titles, alt scroll, in-band resize, synchronized output. F-keys drive it; q quits. |
| `term/queries` | Every query a program may send and dekit's reply. |
| `term/regions` | Scroll regions, left/right margins, IL/DL/ICH/DCH/ECH, DECIC/DECDC, REP, tab stops, DECALN, DEC line drawing, autowrap off, insert mode. |
| `term/sync` | A 30 fps full-screen repaint; `s` toggles `?2026` wrapping. |
| `term/reflow` | Resize the pane: wrapped lines, wide glyphs, an image and the cursor follow. Scroll back while resizing. |
| `term/flood` | 200k styled lines as fast as possible, timed; then scroll back through them. |
| `term/pager` | An alternate-screen pager: the wheel scrolls it through alternate scroll. |
| `term/icat` | `kitty +kitten icat` showing a generated PNG through dekit's graphics support. |
| `term/nvim`, `term/htop`, `term/yazi`, `term/lazygit`, `term/less` | Real programs. |

Things worth trying in every demo: attach a second client with a
different size or terminal, resize, scroll back, enter copy mode and
select, detach and re-attach, and run `dekit screen <task>` to see the
final-screen dump.
