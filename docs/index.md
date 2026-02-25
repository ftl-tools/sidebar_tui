---
layout: home

hero:
  name: Sidebar TUI
  # text: Terminal session manager
  tagline: Organize your terminals and claude agents with a sidebar.
  image:
    src: /logo.svg
    alt: Sidebar TUI Demo
  # actions:
  #   - theme: brand
  #     text: Get Started
  #     link: /guide/
  #   - theme: alt
  #     text: View on GitHub
  #     link: https://github.com/ftl-tools/sidebar-tui
  #     target: _blank

# features:
#   - icon: ⚡
#     title: Fast by default
#     details: Built in Rust. Sub-millisecond response. Sessions persist across restarts with full history.
#   - icon: 🗂
#     title: Workspaces
#     details: Group sessions by project. Switch contexts instantly. Each workspace saves its own view state.
#   - icon: ⌨️
#     title: Keyboard-first
#     details: Every action is a keystroke away. No mouse required — though scroll support is built in.
#   - icon: 🖥
#     title: Full terminal emulation
#     details: Run vim, htop, claude — anything that works in a terminal works here. Alternate screen, mouse, signals.
#   - icon: 🔌
#     title: Shell agnostic
#     details: Works with bash, zsh, fish, or any shell. Sessions inherit your env vars and working directory.
#   - icon: 🔒
#     title: Local-first
#     details: No cloud, no accounts. Your sessions run locally on your machine, managed by a lightweight daemon.
---

## Install

Works on Windows, WSL, Linux, and MacOS.

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
```

## Launch

Then run `sb` to launch.
