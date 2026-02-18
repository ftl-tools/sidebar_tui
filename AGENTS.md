Build and re-install the cli after every functioning batch of work so that users can see your progress as you work.

## Running Tests

**Unit tests** (fast, ~1s):
```bash
cargo test --lib
```

**E2E tests** (always redirect to a file — never pipe):
```bash
cargo test --test e2e -- --test-threads=4 2>/tmp/e2e_out.txt; cat /tmp/e2e_out.txt
```

Single test:
```bash
cargo test --test e2e "test_name_here" -- --nocapture 2>/tmp/e2e_out.txt; cat /tmp/e2e_out.txt
```

The Bash tool's default timeout is 120 seconds. E2E tests — even individual ones — can exceed this. Piping gives you nothing if the process is killed by timeout. Always redirect to a file and read it afterward.
