# Installation

## Requirements

- macOS, Linux, or Windows
- Terminal that supports 256 colors (most modern terminals do)
- Minimum terminal size: 64 columns × 24 rows

## Install methods

::: code-group

```sh [curl]
curl -fsSL https://ftl-tools.github.io/sidebar-tui/install.sh | sh
```

```sh [Homebrew]
brew install ftl-tools/sidebar-tui/sb
```

```sh [npm]
npm install -g sidebar-tui
```

```sh [bun]
bun add -g sidebar-tui
```

```sh [AUR]
paru -S sidebar-tui-bin
# or: yay -S sidebar-tui-bin
```

:::

## Verify

```sh
sb --version
```

## Updating

### curl / binary

```sh
sb self-update
```

### Homebrew

```sh
brew upgrade ftl-tools/sidebar-tui/sb
```

### npm / bun

```sh
npm update -g sidebar-tui
# or
bun update -g sidebar-tui
```

### AUR

```sh
paru -Syu sidebar-tui-bin
```

## Uninstall

Remove the `sb` binary from your `PATH`. If you used npm or bun, run `npm uninstall -g sidebar-tui` or `bun remove -g sidebar-tui`.

Session data is stored in `$XDG_DATA_HOME/sidebar-tui/` (typically `~/.local/share/sidebar-tui/`). Remove this directory to fully clean up.
