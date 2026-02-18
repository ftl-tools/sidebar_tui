Build and re-install the cli after every functioning batch of work so that users can see your progress as you work.

## Running Tests

**Unit tests** (fast, ~1s):
```bash
cargo test --lib
```

**E2E tests** — the full suite takes ~170s with 4 threads. **You MUST set `timeout: 600000` on the Bash tool call**, or the tool will kill the process at 120s and you'll see truncated output that looks like "1 passed; 68 filtered out" — a false signal that makes it look like a filter argument problem. It is not. It is a timeout kill.

Full suite:
```bash
cargo test --test e2e -- --test-threads=4 2>&1 | tee /tmp/e2e_out.txt; grep -E "test result|FAILED|error\[" /tmp/e2e_out.txt
```

Single test:
```bash
cargo test --test e2e "test_name_here" -- --nocapture 2>&1 | tee /tmp/e2e_out.txt; tail -20 /tmp/e2e_out.txt
```

**Critical: use `2>&1 | tee`, not `2>/tmp/e2e_out.txt`.** cargo test prints results to **stdout**, not stderr. Redirecting only stderr (`2>`) leaves the results file nearly empty — you'll see the compile lines but nothing about pass/fail. `2>&1 | tee` captures both streams so the file actually has the results. The `tee` also lets you stream output live while saving to file.

When developing, just run e2e tests related to your feature. Only run the full e2e test suite IF YOU ARE DOING A REVIEW PROCESS. The full suite takes a long time and can be done by a future review agent. If you are a review agent then run the full e2e tests, not just the test for your feature since you want to make sure nothing else was broken in the process.

## E2E Test Infrastructure — Key Pitfalls

### Env vars and the daemon process

Shell sessions inherit their environment from the **daemon process**, not from the `sb` client that connects to it. The daemon is started once (as a background child of the first `sb` invocation) and stays running. Any env vars set only on a later `sb` client call are invisible to new sessions.

This matters when writing tests that check env var inheritance. `TestEnv::setup()` boots the daemon via a bare `list` call — without any custom env vars. If you then spawn `sb` with a custom var and expect it to appear in a new session, it won't.

**Fix:** use `TestIsolation` directly instead of `TestEnv::setup()`, and set your custom env var on the initial `list` call that boots the daemon:

```rust
let iso = TestIsolation::new();
let binary = get_binary_path();

// Boot daemon WITH the custom var so it's in the daemon's environment
let mut cmd = std::process::Command::new(&binary);
iso.apply(&mut cmd);
cmd.arg("list");
cmd.env("MY_VAR", "my_value");
cmd.output().ok();
std::thread::sleep(Duration::from_millis(300));

// Now spawn the TUI — sessions it creates will inherit MY_VAR
```

Remember to call `iso.cleanup()` manually at the end of the test (since you're not using `TestEnv` which does it in `Drop`).
