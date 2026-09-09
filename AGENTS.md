Build and re-install the cli after every functioning batch of work so that users can see your progress as you work.

## Running Tests

Prefer the bounded, process-isolated runner (Python 3.9+, macOS/Linux):

```bash
npm test                              # Fast developer gate, not the full legacy audit
npm run test:unit                     # Library + CLI + compatibility tests
npm run test:tmux                     # Native tmux feature tests
python3 scripts/test_runner.py --suite e2e --filter test_name
npm run test:full                     # Explicit full review/audit; failures are not suppressed
```

The runner builds once with `--locked`, discovers Cargo's artifacts, uses four isolated workers,
prints heartbeat/slow-test progress, and enforces a 30-second per-test watchdog plus a **60-second
whole-run budget** (build + discovery + all tests). Use `--repeat N` for stability checks,
`--timeout N` for a per-case budget, and `--run-timeout N` for an explicitly larger overall budget.
`--build-timeout N` caps compilation separately but cannot extend the whole-run deadline. Logs and machine-readable timings are in `target/test_reports/`. Never silently retry a
failed case. Desktop screenshots are separate: `npm run test:desktop` (macOS, opt-in).

Fast includes the unit/CLI/compatibility/scaffold suites, every tmux integration, current legacy
terminology/throughput workflows and harness regressions. It explicitly excludes the remaining
old legacy UI scenarios; the full audit still reports their stale-binding/layout failures.
See `docs/guide/testing.md` for scope and known limitations.

After a functioning batch, reinstall using `cargo install --path . --force --locked --offline`
when dependencies are cached; this avoids unnecessary index updates and dependency re-resolution.
Keep install separate from test loops. If a cached dependency is missing, retry explicitly without
`--offline`, not by repeatedly running an unbounded combined test/install command.

**Direct unit tests** (normally ~1s, but no per-case watchdog):

```bash
cargo test --lib
```

### Tool timeouts are seconds, not milliseconds

**Never use `timeout: 600000` with this Bash tool.** The old instruction confused milliseconds
with seconds: that value permits almost seven days. A large outer timeout is not a hang fix.

- Fast/focused/unit checks: runner whole-run limit **60s**, Bash `timeout: 90`.
- Explicit full review: runner `--run-timeout 180`, Bash `timeout: 210`.
- If a known cold build needs more time, choose and explain a finite budget explicitly; do not
  automatically inflate deadlines after a timeout. Keep builds/installs separate from test runs.
- The runner emits progress every two seconds while stalled, cancels queued/running tests on
  overall expiry, and exits **124**. Active workers have up to five seconds for scoped cleanup.
- The tool timeout is a final backstop for failures in the runner itself. On any cutoff, inspect
  the saved log/report rather than interpreting partial output or zero tests as success.

Full review (Bash `timeout: 210`):

```bash
set -o pipefail
python3 scripts/test_runner.py --suite e2e --run-timeout 180 2>&1 | tee /tmp/e2e_out.txt
```

Single test (Bash `timeout: 90`):

```bash
set -o pipefail
python3 scripts/test_runner.py --suite e2e --filter test_name_here 2>&1 | tee /tmp/e2e_out.txt
```

**Critical: use `2>&1 | tee`, not `2>/tmp/e2e_out.txt`.** cargo test prints results to **stdout**, not stderr. Redirecting only stderr (`2>`) leaves the results file nearly empty — you'll see the compile lines but nothing about pass/fail. `2>&1 | tee` captures both streams so the file actually has the results. The `tee` also lets you stream output live while saving to file.

When developing, just run e2e tests related to your feature. Only run the full e2e test suite IF YOU ARE DOING A REVIEW PROCESS. The full suite takes a long time and can be done by a future review agent. If you are a review agent then run the full e2e tests, not just the test for your feature since you want to make sure nothing else was broken in the process.

## E2E Test Infrastructure — Key Pitfalls

### Env vars and the server process

Window shells inherit their environment from the **server process**, not from the `sb` client that connects to it. The server is started once (as a background child of the first `sb` invocation) and stays running. Any env vars set only on a later `sb` client call are invisible to new windows.

This matters when writing tests that check env var inheritance. `TestEnv::setup()` boots the server via a bare `list` call — without any custom env vars. If you then spawn `sb` with a custom var and expect it to appear in a new window, it won't.

**Fix:** use `TestIsolation` directly instead of `TestEnv::setup()`, and set your custom env var on the initial `list` call that boots the server:

```rust
let iso = TestIsolation::new();
let binary = get_binary_path();

// Boot server WITH the custom var so it's in the server's environment
let mut cmd = std::process::Command::new(&binary);
iso.apply(&mut cmd);
cmd.arg("list");
cmd.env("MY_VAR", "my_value");
cmd.output().ok();
std::thread::sleep(Duration::from_millis(300));

// Now spawn the TUI — windows it creates will inherit MY_VAR
```

Remember to call `iso.cleanup()` manually at the end of the test (since you're not using `TestEnv` which does it in `Drop`).

## Project Knowledge and Handoffs

Use checked-in Markdown, not Mulch. The previous mandatory CLI workflow repeatedly failed
because the executable was not installed; it is no longer a project prerequisite. Do not run
or install Mulch as part of normal agent startup, validation, or completion.

- At session start, read the relevant project instructions and task handoff. For the tmux
  migration, read `docs/guide/tmux_migration_progress.md`; for tests, read `docs/guide/testing.md`.
- Before finishing, record useful decisions, pitfalls, exact validation results, and unresolved
  work in the relevant guide/handoff (or `progress.md` for work without a dedicated handoff).
  Make targeted updates and preserve other agents' notes. Include test names or commit IDs
  when available; do not commit or push merely to record knowledge.
- `.mulch/` is a preserved historical archive, not the active workflow. Read its JSONL records
  directly only when relevant, and verify them against current code: some describe retired
  bindings, terminology, and tooling. Do not treat archived records as current instructions.
- For documentation changes, run `npm run docs:build`; no knowledge-management CLI is needed.
