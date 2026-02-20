# npm Binary Distribution Research

Research for issue sidebar_tui-8fqc: how to distribute the `sb` CLI binary via npm/bun.

## Summary

There are two main patterns for distributing a Rust CLI binary via npm:

1. **optionalDependencies pattern** (recommended): Main package lists platform-specific packages as `optionalDependencies`. npm/bun automatically installs only the one matching the current platform (based on `os` and `cpu` fields in each platform package's `package.json`). The main package ships a `bin/sb` Node.js launcher script that resolves the correct platform binary and spawns it.

2. **postinstall download pattern**: A single npm package with a `postinstall` script that detects the platform and downloads the correct binary from GitHub Releases. Used by cargo-dist's npm installer template. Simpler to set up but slower to install (requires a download at install time) and fails in offline/air-gapped environments.

**Recommendation for sidebar-tui**: Use pattern 1 (optionalDependencies). It's what esbuild, biome, turbo, and all major tools use. It's faster, works offline once installed, and is more reliable.

## Pattern 1: optionalDependencies (esbuild/biome style)

### Package structure

```
npm/
  sidebar-tui/              # main package, published as "sidebar-tui" on npm
    package.json
    bin/sb                  # Node.js launcher script
  sidebar-tui-darwin-arm64/ # platform-specific package
    package.json            # has "os": ["darwin"], "cpu": ["arm64"]
    sb                      # the actual binary
  sidebar-tui-darwin-x64/
    package.json
    sb
  sidebar-tui-linux-x64/
    package.json
    sb
  sidebar-tui-win32-x64/
    package.json
    sb.exe
```

### Main package.json (sidebar-tui)

```json
{
  "name": "sidebar-tui",
  "version": "0.2.0",
  "description": "A terminal UI sidebar for managing Claude sessions",
  "bin": {
    "sb": "bin/sb"
  },
  "scripts": {
    "postinstall": "node bin/sb --version 2>/dev/null || true"
  },
  "optionalDependencies": {
    "sidebar-tui-darwin-arm64": "0.2.0",
    "sidebar-tui-darwin-x64": "0.2.0",
    "sidebar-tui-linux-x64": "0.2.0",
    "sidebar-tui-linux-arm64": "0.2.0",
    "sidebar-tui-win32-x64": "0.2.0"
  },
  "engines": {
    "node": ">=16"
  },
  "license": "MIT"
}
```

Note: The `postinstall` script is optional. esbuild uses one to do a verification step; biome omits it.

### Platform package.json (sidebar-tui-darwin-arm64)

```json
{
  "name": "sidebar-tui-darwin-arm64",
  "version": "0.2.0",
  "description": "The macOS ARM64 binary for sidebar-tui.",
  "os": ["darwin"],
  "cpu": ["arm64"],
  "preferUnplugged": true,
  "license": "MIT"
}
```

The `preferUnplugged: true` tells yarn/pnpm to extract the package to disk rather than keeping it zipped (required for the binary to be executable).

The `os` and `cpu` fields tell npm/bun to skip this package if the current platform doesn't match. npm installs it as an optionalDependency, so if it can't be installed (wrong platform), the overall install still succeeds.

### Launcher script (bin/sb)

```js
#!/usr/bin/env node

const { platform, arch } = process;

const PLATFORMS = {
  darwin: {
    x64: "sidebar-tui-darwin-x64/sb",
    arm64: "sidebar-tui-darwin-arm64/sb",
  },
  linux: {
    x64: "sidebar-tui-linux-x64/sb",
    arm64: "sidebar-tui-linux-arm64/sb",
  },
  win32: {
    x64: "sidebar-tui-win32-x64/sb.exe",
  },
};

// Allow override for development/CI
const binPath = process.env.SB_BINARY || PLATFORMS[platform]?.[arch];

if (!binPath) {
  console.error(
    `sidebar-tui: no binary available for ${platform}/${arch}. ` +
    `Install from https://github.com/your-repo for your platform.`
  );
  process.exit(1);
}

const result = require("child_process").spawnSync(
  require.resolve(binPath),
  process.argv.slice(2),
  { shell: false, stdio: "inherit" }
);

if (result.error) throw result.error;
process.exitCode = result.status;
```

This script is what goes in `bin/sb` in the main package. It delegates to the platform binary via `require.resolve()` which looks up the binary in `node_modules`.

Key design decisions from studying biome/esbuild:
- `SB_BINARY` env var allows overriding for dev/CI (biome uses `BIOME_BINARY`)
- `shell: false` + `stdio: "inherit"` is important for proper terminal behavior (TUI apps need this)
- `require.resolve()` finds the binary in node_modules without needing to know the exact path

### How bun handles it

`bun add -g sidebar-tui` works the same way as npm - bun respects `os`/`cpu` fields in optionalDependencies and installs only the matching platform package. The launcher script works identically since bun can run Node.js scripts.

### GitHub Actions workflow for publishing

The publish workflow needs to:
1. Build binaries for each platform (can use matrix builds or cross-compilation)
2. Copy each binary into the correct platform package directory
3. `npm publish` each platform package
4. `npm publish` the main package last

See `biome/.github/workflows/release.yml` for a full working example with matrix builds across macOS, Linux, and Windows runners.

The key step from biome's workflow:
```yaml
- name: Generate npm packages
  run: node packages/@biomejs/biome/scripts/generate-packages.mjs

- name: Publish npm packages as latest
  run: for package in packages/@biomejs/*; do npm publish $package --tag latest --access public; fi
```

### Package naming

Options for sidebar-tui:
- `sidebar-tui` + `sidebar-tui-darwin-arm64` etc. (flat naming, like turbo)
- `@your-scope/sidebar-tui` + `@your-scope/cli-darwin-arm64` etc. (scoped, like biome)

Flat naming (no scope) is simpler for `npm install -g sidebar-tui`. Scoped naming requires users to do `npm install -g @your-scope/sidebar-tui`.

Since the spec says `npm install -g sidebar-tui`, use flat naming.

## Pattern 2: postinstall download (cargo-dist style)

Used by cargo-dist's npm installer template. The single package downloads the binary at install time.

See `cargo-dist/cargo-dist/templates/installer/npm/binary.js` for the full implementation.

Key parts:
- `package.json` has a `postinstall` script that calls `node ./install.js`
- `install.js` detects platform, downloads the correct binary from GitHub Releases, and makes it executable
- Uses `axios` for downloading, `detect-libc` for Linux libc detection

**Downsides**:
- Requires internet access at install time
- Slower install (downloads ~5-10MB binary)
- More failure modes (network errors, GitHub rate limiting)
- Harder to verify/audit

**When to use**: Only if you can't publish multiple packages to npm (e.g., no npm account/org).

## Local Reference Files

- `esbuild/` - Full esbuild repo (Go CLI, but canonical npm distribution example)
  - `esbuild/npm/esbuild/package.json` - Main package with optionalDependencies
  - `esbuild/npm/@esbuild/darwin-arm64/package.json` - Platform package structure
- `biome/` - Full biome repo (Rust CLI, most directly applicable)
  - `biome/packages/@biomejs/biome/package.json` - Main package
  - `biome/packages/@biomejs/biome/bin/biome` - Launcher script
  - `biome/packages/@biomejs/biome/scripts/generate-packages.mjs` - Script to copy binaries into npm packages
  - `biome/packages/@biomejs/cli-darwin-arm64/package.json` - Platform package
  - `biome/.github/workflows/release.yml` - Full release + publish workflow
- `cargo-dist/` - cargo-dist tool (has npm postinstall download pattern)
  - `cargo-dist/cargo-dist/templates/installer/package.json` - Template postinstall package.json
  - `cargo-dist/cargo-dist/templates/installer/npm/binary.js` - Platform detection + download

## Key Decisions for sidebar-tui

1. **Use optionalDependencies pattern** (not postinstall download)
2. **Package name**: `sidebar-tui` (flat, matches spec's `npm install -g sidebar-tui`)
3. **Binary name in package**: `sb` (matches the CLI binary name)
4. **Platforms to support**: macOS arm64, macOS x64, Linux x64, Linux arm64, Windows x64
5. **Environment variable override**: `SB_BINARY` for dev/CI use
6. **Publish order**: platform packages first, main package last
7. **Version sync**: all packages must have identical versions

## Example install commands that work

```sh
npm install -g sidebar-tui
bun add -g sidebar-tui
npx sidebar-tui  # works without global install too
```
