---
title: dekit runner restart
cli: dekit runner restart
order: 8
related: [cli/runner, cli/runner/upgrade, cli/restart, cli/reload]
---

Restarts the runner in place: running tasks keep running, and every task
comes back as it was, edits made in the console and the list's order
included. `dekit.yaml` is not read for the tasks; |cli/reload| takes what
changed there. To restart tasks instead, use |cli/restart|. Not available
on Windows.

:::usage
