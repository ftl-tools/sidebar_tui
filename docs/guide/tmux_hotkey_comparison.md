# Sidebar and tmux hotkey comparison

## Shared terminology

Sidebar now uses tmux's hierarchy: **sessions** contain **windows**, each currently with one **terminal pane**. The sidebar is a window picker, not a pane. Split panes are not implemented.

This is terminology alignment only: Sidebar still uses its own PTY server, not tmux. The table below preserves the earlier hotkey proposal for comparison; the implemented bindings are documented in [keybindings](./keybindings.md).

## Proposal: the sidebar is both prefix mode and window tree

**Implemented:** these bindings now define Sidebar's default interaction model. Command-key aliases still depend on terminal/OS support as described below.

Instead of entering an invisible, one-command prefix mode, toggle focus into the sidebar. It is a persistent, visible command mode and window picker at the same time.

**Toggle** means any of `Ctrl+Space`, `Ctrl+B`, `Cmd+Space`, or `Cmd+B`. Command-key aliases are conditional on OS/terminal support (see caveats below).

- From the terminal, Toggle reveals/unzooms the sidebar and focuses it, initially highlighting the active window.
- From the sidebar, Toggle focuses the highlighted terminal, just like `Enter`. It does not require a second `w` key.
- `↑`/`↓` and `j`/`k` browse windows with live preview. Commands such as `r` act on the **highlighted** window, not the previously active one.
- `Esc` or `q` cancels browsing and restores the window active when browsing began. Unlike Toggle/Enter, this does not commit the highlighted selection.
- Bare command letters work only while the sidebar is focused. For example, `c` starts terminal creation and `s` opens the session chooser.
- Navigation, renaming, moving, and deletion leave focus in the sidebar. Successful creation focuses the new terminal. `Enter`, Toggle, `l`, and `z` return focus to a terminal; `d` detaches after confirmation.
- Session actions `C`, `R`, and `K` target the current session, or the highlighted session when its chooser is open. In that chooser, `↑`/`↓`, `j`/`k`, and `Enter` navigate/select; `Esc`/`q` closes it back to the sidebar. Other window commands are disabled there.
- Naming fields and confirmation dialogs take precedence over command bindings: letters are text while naming; `Enter` submits and `Esc` cancels. Toggle aliases do not switch focus away from an unfinished dialog.
- Direct Alt shortcuts work from the terminal or sidebar in normal browsing mode, not while typing into dialogs. From the terminal they switch immediately; from the sidebar they update the selection while retaining sidebar focus.

This makes `Toggle → c` feel like an Omarchy prefix command, while `Toggle → j → j → r` lets you browse and rename without another prefix. Commands remain available after each navigation step; there is no prefix timeout.

**Not every tmux binding can be merged unchanged:** `k` cannot simultaneously mean “up” and “kill window.” The proposal prioritizes safe tree navigation, preserving compatible Omarchy commands and explicitly remapping collisions below.

## Complete comparison

In tmux columns, **P** means the tmux prefix: stock `Ctrl+B`; Omarchy `Ctrl+Space` or `Ctrl+B`. `P → c` means press/release the prefix, then press lowercase `c`. Uppercase letters such as `C` mean `Shift+c`.

In the proposed column, **bare keys are sidebar-focused commands**, not global shortcuts. **Toggle** is defined above. Current Sidebar bare keys also require sidebar focus unless another context is stated.

| Task | Sidebar (current) | Sidebar (proposed) | Stock tmux | Omarchy tmux |
|---|---|---|---|---|
| Focus/open window list | `Ctrl+B` (also `Ctrl+T`) | **Toggle**; `w` while already focused stays in the window list | `P → w` | `P → w` |
| Browse/select window | `↑`/`↓` or `j`/`k`, then `Enter` | `↑`/`↓` or `j`/`k`, then `Enter` or Toggle | Window tree: `↑`/`↓`, then `Enter` | Window tree: `↑`/`↓` or `j`/`k`, then `Enter` |
| Cancel window browsing | `Esc`, `b`, or `Ctrl+B` restores last active window | `Esc` or `q` restores pre-browse window; Toggle instead commits selection | Tree: `Esc` or `q` | Tree: `Esc` or `q` |
| List windows from shell | Sidebar provides a visible list | No CLI change proposed | `tmux list-windows` | Same |
| List all windows across sessions | No direct cross-session list | No new aggregate view proposed; `s` selects session, then browse its windows | `tmux list-windows -a` | Same |
| Create terminal/window | `Ctrl+N`, then `t`; or `n`, then `t` | `c`, optional name, then `Enter`; no terminal/agent type prompt | `P → c` | `P → c`; active pane's directory |
| Create agent terminal | `Ctrl+N`, then `a`; or `n`, then `a` | `a`, optional name, then `Enter` | No built-in equivalent | No built-in equivalent |
| Select numbered window | No direct number binding | `1`–`9`, then `Enter`; direct `Alt+1`–`Alt+9` also available | `P → 0`–`9` | Direct `Alt+1`–`Alt+9`; prefix-number also works; numbering starts at 1 |
| Next/previous window | Browse sidebar | `n`/`p` selects next/previous; direct `Alt+Right`/`Alt+Left` | `P → n`/`p` | Direct `Alt+Right`/`Alt+Left`; prefix bindings retained |
| Last active window | Sidebar jump-back keys | `l` switches to previously active window and focuses terminal | `P → l` | `P → l` |
| Focus highlighted terminal | `Enter`, `Space`, `→`, or `Tab` | `Enter` or Toggle | Tree selection, then `Enter` | Same |
| Rename window | `r` | `r` (also `,`), edit name, then `Enter` | `P → ,` | `P → r`; comma alias retained |
| Delete window | `d`, then confirm | `&` or `Delete`, then confirm; **never `k`** | `P → &`, confirm | `P → k` immediately; `P → &` with confirmation |
| Move window to session | `m`, choose session, `Enter` | `m`, choose session, `Enter` | `tmux move-window`; no default direct binding | Same |
| Reorder window | No binding | Direct `Alt+Shift+Left`/`Alt+Shift+Right` (new capability) | No matching direct default binding | Direct `Alt+Shift+Left`/`Alt+Shift+Right` |
| Open session chooser | `w` or `Ctrl+W` | `s`; choose with arrows or `j`/`k`, then `Enter` | `P → s` | `P → s` |
| Create session | Session overlay: `n` | `C`, name, then `Enter` | `tmux new-session`; no default prefix binding | `P → C` |
| Previous/next session | Choose in session overlay | `P`/`N`; direct `Alt+Up`/`Alt+Down` | `P → (`/`)` | Direct `Alt+Up`/`Alt+Down`; `P → P`/`N` |
| Rename session | Session overlay: `r` | `R` (also `$`), edit name, then `Enter` | `P → $` | `P → R`; dollar alias retained |
| Delete session | Session overlay: `d`, confirm | `K`, then confirm with session name and affected terminals shown | `tmux kill-session` | `P → K` immediately |
| Hide sidebar / zoom | Terminal: `Ctrl+Z` | `z` hides sidebar and focuses terminal; Toggle reveals it again | `P → z` zooms active pane | Same |
| Toggle mouse/text selection | `Ctrl+S` | `S` (Shift+s); retain this Sidebar-specific capability without intercepting terminal `Ctrl+S` | No default toggle hotkey | Mouse on by default; no toggle binding |
| Split window into panes | Not supported | Not proposed; leave pane shortcuts unassigned | `P → "` top/bottom; `P → %` left/right | `Alt+Enter` / `Alt+Shift+Enter`; `P → h` / `v` |
| Move among panes | Not supported | Not proposed; arrows browse windows instead | `P → arrow`; `P → o` next pane | Direct `Ctrl+Alt+arrow`; stock bindings retained |
| Resize pane | Not supported | Not proposed | `P → Ctrl+arrow` or `Alt+arrow` | Direct `Ctrl+Alt+Shift+arrow` resizes five cells |
| Kill pane | Not supported | Not proposed; **do not alias `x` or `Alt+Esc` to deleting a window** | `P → x`, confirm | `P → x` or direct `Alt+Esc`, **without confirmation** |
| Detach; leave terminals running | `Ctrl+Q` or sidebar `q`, confirm | `d`, then confirm; `q` only cancels browsing | `P → d` | Same |
| Copy/scrollback mode | Mouse scrolling/selection; no tmux-style copy mode | Keep mouse scrolling/selection; reserve `[` for potential copy mode, not part of migration | `P → [` | Same; vi copy mode `v` starts selection, `y` copies |
| Reload configuration | No equivalent | Not proposed; `q` cancels browsing, not reload | `P → :`, then `source-file ~/.tmux.conf` | `P → q` |
| Keybinding help | Context-aware hint bar | `?` shows sidebar command help (new capability) | `P → ?` | Inherited stock binding |

## Conflicts and compatibility decisions

These are deliberate departures, not accidental omissions from Omarchy. tmux normally separates its prefix table, window-tree controls, copy mode, and direct shortcuts; merging them introduces collisions.

| Keys / feature | Why the merge is problematic | Proposed resolution |
|---|---|---|
| `k` | Vi tree “up” versus Omarchy prefix “kill window” | Always navigate up. Delete with `&` or `Delete` and confirmation. Do not infer intent from speed or whether the sidebar was just opened. |
| `h`, `l`, arrows | Vi tree collapse/expand versus Omarchy `h` split; stock prefix arrows select panes and `l` selects last window | Sidebar is a flat window list scoped to a session. Up/down browse; `l` means last window. No collapse/expand or pane commands. A future hierarchical tree needs a separate key design. |
| `q`, `d` | Tree `q` exits, Omarchy prefix `q` reloads; old Sidebar `q` quits and `d` deletes | `q` cancels browsing; `d` detaches with confirmation. Update hints and remove conflicting old aliases. |
| `n`, `p`, `N`, `P` | Old Sidebar `n` creates; tmux prefix `n`/`p` navigates windows; Omarchy uppercase variants navigate sessions | Create with `c`/`C`; navigate with `n`/`p` and `N`/`P`. Typing uppercase must not accidentally invoke lowercase commands. |
| `r`, `R`, `s`, `w` | tmux's window tree has its own refresh/sort/view commands, distinct from prefix commands | Give prefix semantics priority: rename window/session, choose session/window list. Do not claim complete tmux tree-key compatibility. |
| `m`, `M`, tree tagging | tmux prefix `m` marks a pane; tree tagging can select multiple items for operations | Keep Sidebar `m` for moving one highlighted window. No pane marks or multi-selection; do not implement tag-and-kill operations implicitly. |
| `1`–`9`, `Space`, `Enter` | Tree shortcut labels may select tree entries, not literal window numbers; stock prefix Space cycles layouts | Use stable displayed 1-based window positions. Bare digits highlight, Enter commits. Leave Space unassigned in browsing mode; no layout cycling. |
| `x`, `K`, `Alt+Esc` | Omarchy directly kills panes/sessions without confirmation | No pane shortcuts in Sidebar; session `K` always confirms. Current Omarchy explicitly overrides `x` with `kill-pane`, removing stock confirmation. |
| `[`, `]`, `v`, `y`, `=`, `#` | tmux copy/paste/buffer commands depend on modes and buffer infrastructure Sidebar does not have | Reserve rather than pretending these work. No new clipboard/copy subsystem in a hotkey migration. |
| `:`, `!`, `%`, `"`, `o`, `;`, `{`, `}`, layout keys | Stock bindings inherited by Omarchy invoke tmux commands, split/break/select/swap panes, or change layouts | Unsupported here. While sidebar-focused, consume unsupported keys without sending them to the shell; show a hint when appropriate. They pass through normally when terminal-focused unless explicitly bound globally. |
| Prefix twice | Omarchy `Ctrl+Space`, then `Ctrl+Space` sends a literal prefix to the child | Sidebar Toggle twice enters then leaves sidebar; **not** send-prefix. A separate configurable send-key action would be required for literal prefix forwarding. |
| Sticky command mode | tmux returns to terminal input after most prefix commands; Sidebar remains focused after navigation/editing | Keep a clear focus indicator and contextual hints. Never forward unused sidebar commands to the child terminal. |

### OS, terminal, and nesting constraints

- **Cmd aliases are best-effort, not portable raw terminal keys.** macOS commonly reserves `Cmd+Space` for Spotlight; terminal apps may capture `Cmd+B`. Users must release conflicting OS/app shortcuts and either enable a keyboard protocol the app supports or map these chords to `Ctrl+Space`/`Ctrl+B` in terminal settings. `Ctrl+Space` may also be an input-source/IME shortcut.
- **Ctrl+Space commonly arrives as NUL (`0x00`).** Implementation must recognize the terminal's actual decoded event, including legacy encoding, rather than assuming every terminal delivers a distinct modifier event. Verify all aliases across supported terminals.
- **Nested tmux has two directions of conflict.** If tmux contains Sidebar, outer tmux consumes matching prefixes and direct Alt shortcuts first. If Sidebar contains tmux, Sidebar consumes its configured global shortcuts before the child sees them. Remapping/configurable interception or a deliberate passthrough mode is needed; identical global bindings cannot operate both layers simultaneously. Using Cmd aliases mapped to those same bytes does not avoid this.
- **Direct Alt shortcuts conflict with terminal/editor bindings.** `Alt+Left/Right` often moves by words; macOS Option may produce characters instead of Alt escape sequences. Make these shortcuts configurable/disableable and test shifted arrows with terminal keyboard-protocol support. Bare sidebar commands remain the fallback.
- **Return old control keys to the terminal.** Retire global `Ctrl+N`, `Ctrl+W`, `Ctrl+S`, `Ctrl+Z`, `Ctrl+Q`, and the old `Ctrl+T` alias rather than silently retaining them. The four requested Toggle aliases are the intended focus shortcuts. This restores common shell/editor operations, but `Ctrl+B` remains intentionally intercepted and should be configurable.

## Example proposed workflows

| Task | Starting in terminal |
|---|---|
| Create terminal | Toggle, `c`, optional name, `Enter` |
| Browse and rename another terminal | Toggle, `j`, `j`, `r`, edit name, `Enter`; remain in sidebar |
| Browse and focus another terminal | Toggle, `j`, `Enter` (or Toggle again) |
| Preview without switching | Toggle, `j`, `j`, `Esc` |
| Delete highlighted terminal safely | Toggle, navigate, `&`, confirm |
| Switch session | Toggle, `s`, navigate, `Enter`; browse its windows |
| Create session | Toggle, `C`, name, `Enter` |
| Switch directly without opening sidebar | `Alt+1`–`Alt+9` or `Alt+Left/Right` |
| Toggle mouse capture | Toggle, `S` |
| Detach | Toggle, `d`, confirm |

## Scope

This document proposes bindings and interaction semantics, not a tmux backend migration. Direct numbered switching, reordering, help, and the sticky command-mode routing need implementation and tests. No new panes, copy mode, multi-selection, or cross-session aggregate tree are included.

## Sources

- [Omarchy tmux configuration](https://github.com/basecamp/omarchy/blob/master/config/tmux/tmux.conf) — upstream master; bindings may change with releases or user overrides.
- [tmux manual](https://man.openbsd.org/tmux) — default prefix bindings and `choose-tree` mode controls.
- Sidebar current implementation: `src/input_handler.rs` and [keybindings](./keybindings.md).
