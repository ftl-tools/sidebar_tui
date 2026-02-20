# Turbo npm Package Structure (fetched from npm registry)

These are examples fetched from the npm registry for turbo v2.8.10.

## Main package: turbo

```json
{
  "name": "turbo",
  "version": "2.8.10",
  "bin": {
    "turbo": "bin/turbo"
  },
  "optionalDependencies": {
    "turbo-linux-64": "2.8.10",
    "turbo-darwin-64": "2.8.10",
    "turbo-windows-64": "2.8.10",
    "turbo-linux-arm64": "2.8.10",
    "turbo-darwin-arm64": "2.8.10",
    "turbo-windows-arm64": "2.8.10"
  }
}
```

Note: turbo uses `-64` instead of `-x64` and doesn't use scoped packages (no `@vercel/` prefix).

## Platform package: turbo-darwin-arm64

```json
{
  "name": "turbo-darwin-arm64",
  "version": "2.8.10",
  "license": "MIT",
  "description": "The darwin-arm64 binary for turbo, a monorepo build system.",
  "homepage": "https://turborepo.dev",
  "repository": {
    "url": "git+https://github.com/vercel/turborepo.git",
    "type": "git"
  },
  "os": ["darwin"],
  "cpu": ["arm64"],
  "preferUnplugged": true
}
```

## Key observations

- turbo uses flat (unscoped) package names: `turbo-darwin-arm64`
- The `preferUnplugged: true` is important for yarn/pnpm berry - without it they may keep the package zipped which prevents the binary from being executed
- No `bin` field in the platform packages - only the main package has the `bin` field pointing to the launcher script
- The launcher script (at `bin/turbo`) detects the platform, finds the right platform package binary, and spawns it with `execFileSync`
- turbo's launcher also handles JIT installation (if the platform package wasn't installed, it tries to install it on-the-fly) and emulation fallback (ARM64 Mac can run x64 binaries via Rosetta)
