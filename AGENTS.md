Build and re-install the cli after every functioning batch of work so that users can see your progress as you work.

## Running Tests

**Unit tests** (fast, ~1s):
```bash
cargo test --lib
```

**E2E tests** — the full suite takes ~170s with 4 threads. **You MUST set `timeout: 600000` on the Bash tool call**, or the tool will kill the process at 120s and you'll see truncated output that looks like "1 passed; 68 filtered out" — a false signal that makes it look like a filter argument problem. It is not. It is a timeout kill.

Full suite:
```bash
cargo test --test e2e -- --test-threads=4 2>/tmp/e2e_out.txt; cat /tmp/e2e_out.txt
```

Single test:
```bash
cargo test --test e2e "test_name_here" -- --nocapture 2>/tmp/e2e_out.txt; cat /tmp/e2e_out.txt
```

Always redirect to a file (`2>/tmp/e2e_out.txt`) rather than piping. If the process is killed mid-run, a pipe gives you nothing; a file at least has partial output you can inspect. But the real fix is setting `timeout: 600000` so the process is never killed in the first place.
