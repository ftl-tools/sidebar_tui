Build and re-install the cli after every functioning batch of work so that users can see your progress as you work.

## Running Tests

**Unit tests** (fast, ~1s):
```bash
cargo test --lib
```

**E2E tests** (slow, ~165s — always set an explicit timeout):
```bash
cargo test --test e2e -- --test-threads=4 2>/tmp/e2e_out.txt; cat /tmp/e2e_out.txt | tail -5
```

The E2E suite takes ~165 seconds. The Bash tool's default timeout is 120 seconds, so without redirecting output to a file the process gets killed silently before finishing. Redirect to a file, then read the file afterward. A single specific test runs in ~6s and can be piped normally:

```bash
cargo test --test e2e "test_name_here" -- --nocapture 2>&1 | tail -20
```
