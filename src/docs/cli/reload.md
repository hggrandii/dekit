---
title: dekit reload
cli: dekit reload
order: 4
related: [cli/down, cli/up, config/tasks, config]
---

Brings what changed in `dekit.yaml` into a running project. A runner
reads the config when it starts fresh; after that the tasks are its own
state: a restart, `down` and `up`, and an upgrade bring them back as
they were, edits made in the console included, and take nothing new
from the config. `reload` is how the config gets in.

The runner remembers what the config said for each task when it last
took the config in: at a reload, and when the console wrote back to it.
Against that, a reload sorts every difference:

- Changed in the config only: taken.
- Changed here only: kept; the console can write it back.
- Changed on both sides: kept as here, unless you pick the config's.
- New in the config: added idle; `up` starts it if it autostarts.
- Gone from the config: removed, unless it runs or was changed here.
- Removed here only: left out; the console can write it back.
- Added here and written into the config as it is: a config task now.

Without `--yes` it only shows that list. With it, it takes the changes
it marked to take. A changed definition applies at the task's next
start: restart a running one to use it. The console's reload dialog
(the badge in the header) picks change by change.

:::usage

```sh
dekit reload
dekit reload --yes
dekit reload --json
```
