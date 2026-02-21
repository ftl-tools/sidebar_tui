## Issue Tracking

This project uses **bd (beads)** for issue tracking.
Run `bd prime` for workflow context, or install hooks (`bd hooks install`) for auto-injection.

**Quick reference:**

- `bd ready` - Find unblocked work
- `bd create "Title" --type task --priority 2` - Create issue
- `bd close <id>` - Complete work
- `bd sync` - Sync with git (run at session end)

For full workflow details: `bd prime`

## Project Expertise (Mulch)

This project uses [Mulch](https://github.com/jayminwest/mulch) for structured expertise management.

**At the start of every session**, run:

```bash
mulch prime
```

This injects project-specific conventions, patterns, decisions, and other learnings into your context.
Use `mulch prime --files src/foo.ts` to load only records relevant to specific files.

**Before completing your task**, review your work for insights worth preserving — conventions discovered,
patterns applied, failures encountered, or decisions made — and record them:

```bash
mulch record <domain> --type <convention|pattern|failure|decision|reference|guide> --description "..."
```

Link evidence when available: `--evidence-commit <sha>`, `--evidence-bead <id>`

Run `mulch status` to check domain health and entry counts.
Run `mulch --help` for full usage.
Mulch write commands use file locking and atomic writes — multiple agents can safely record to the same domain concurrently.

### Before You Finish

1. Discover what to record:
   ```bash
   mulch learn
   ```
2. Store insights from this work session:
   ```bash
   mulch record <domain> --type <convention|pattern|failure|decision|reference|guide> --description "..."
   ```
3. Validate and commit:
   ```bash
   mulch sync
   ```
