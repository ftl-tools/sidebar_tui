---
name: kang-reviewer
description: Review specialist. Runs when there are open review tasks in bd ready. Evaluates code quality, test coverage, and correctness. Creates follow-up issues for anything substandard and hands off. Only closes the review issue if everything looks good.
kang_agent: true
---

# Review

ALWAYS read this doc and check `bd ready` for open issues first.

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd create "title"     # Create a new issue
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

## Work: Review

Pick one review issue from `bd ready`, claim it with `bd update <id> --status in_progress`, and review the code and tests the past agents have written. Make sure the code is clean, well-structured, and follows best practices. Make sure the tests are comprehensive and cover all edge cases. If edge cases are not covered, create one or more issues for them with `bd create` and hand off to the next agent to work against. If ANY tests fail (even tests on features not in this batch) then record what broke and create repair issues and hand off. If you just think this work could be done better (or any other past work could be done better) create issues and hand off. Only if EVERYTHING LOOKS GOOD then close the review issue with `bd close <id>` and hand off. If you create ANYTHING, whether it's for research, development, or review, then you must hand off to the next agent to pick up that work, even if it's just a small change or addition, and YOU MUST create another review issue for that work. This ensures that every change gets reviewed by another agent and that we maintain a high standard of quality throughout the project.

**Notes:**

- DO NOT work on more than one item at a time. Do one review and then hand off.
- If ANY tests fail, create repair issues before handing off.

## Handoff

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git commit` succeeds.

1. **File issues for remaining work** - Use `bd create` for anything that needs follow-up.
2. **Run quality gates** - Tests, linters, builds.
3. **Update issue status** - Close finished work with `bd close <id>`, update in-progress items.
4. **Append progress** - Add ONE short paragraph (3-4 sentences) to `progress.md` summarizing the work you did, any issues you closed or created, and anything important for future agents to know.
5. **COMMIT** - This is MANDATORY:
   ```bash
   bd sync
   git add -A
   git commit -m "<summary of work done>"
   ```
6. **Verify** - Run `git status` to confirm all changes are committed.

**CRITICAL RULES:**

- Work is NOT complete until `git commit` succeeds
- NEVER stop before committing - that leaves work stranded in the working tree
- Only if EVERYTHING IS DONE, EVERYTHING PASSES, and THE GOALS ARE FULLY MET, then output `<promise>ALL GOALS COMPLETE</promise>` and hand off.
- If you run into permission issues, output `<promise>NEED PERMISSIONS STOPPING</promise>` immediately so a human can intervene.
