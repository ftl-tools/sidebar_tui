# Historical project knowledge archive

Mulch is **not an active project dependency**. Its mandatory agent commands repeatedly failed
because the executable was unavailable. The project chose checked-in Markdown handoffs instead
of installing or requiring the CLI. See [AGENTS.md](../AGENTS.md#project-knowledge-and-handoffs).

Do not run initialization, priming, recording, or synchronization commands for this directory.
Preserve the existing files as historical reference:

- `expertise/` — original JSONL records, readable directly without any tool.
- `mulch.config.yaml` — archived configuration, retained alongside those records.

Records may describe obsolete bindings, names, or tooling; verify them against current code and
guides rather than treating them as instructions. Write new findings in the relevant Markdown
guide/handoff, or `progress.md` when no dedicated handoff exists. No archive data was deleted.
