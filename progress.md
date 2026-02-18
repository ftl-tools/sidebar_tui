# Progress Logs

## 2026-02-16 - Compressed History (Early E2E Stability and Performance Work)

Earlier work on 2026-02-15 and 2026-02-16 covered performance optimizations and E2E test stability. Performance work included: Cow-based String allocation reduction, on-demand screen capture instead of per-process(), VecDeque for O(1) history trimming, render batching via socket draining, dirty tracking for incremental rendering, and terminal foreground color fix for Apple Terminal. E2E test infrastructure was significantly hardened: added `ensure_daemon_ready()` polling helper, `spawn_sb()` wrapper, aggressive `cleanup_test_sessions()`, and replaced fixed sleeps with polling loops throughout; this resolved multiple flaky tests including `test_tab_focuses_terminal`, `test_hint_bar_context`, `test_quit_confirmation`, `test_sidebar_is_28_chars_wide`, and `test_welcome_state_on_fresh_start`. Also implemented text selection mode toggle (Ctrl+S), auto-generated three-word session names, and fixed broken mouse scroll tests.

## 2026-02-16 - Fixed Flaky test_hint_bar_context and test_quit_confirmation E2E Tests

Fixed two E2E tests that failed when running the full test suite due to leftover sessions and timing issues. Root cause: (1) `test_quit_confirmation` wasn't calling `cleanup_test_sessions()` at start, (2) `test_hint_bar_context` was using fixed 1500ms sleep which wasn't sufficient under test suite load - the hint bar wasn't visible yet when assertion ran. Fixes: (1) Added `cleanup_test_sessions()` call at the start of `test_quit_confirmation`, (2) Changed `test_hint_bar_context` to use a polling loop (10 x 200ms = 2 seconds max) waiting for hint bar content to appear before asserting. All 365 lib + 65 bin + 35 E2E tests pass. Binary reinstalled.

## 2026-02-16 - Added E2E Test for Vim j/k Navigation

Completed sidebar_tui-s9e: Added E2E test `test_vim_jk_navigation` to verify that vim-style j/k keys work for navigating the session list in the sidebar. The test creates two sessions, focuses the sidebar, then verifies that 'j' moves selection down and 'k' moves selection up, matching the spec requirement "↑ or k - Up, ↓ or j - Down". All 365 lib + 65 bin + 36 E2E tests pass. Binary reinstalled. Closed sidebar_tui-s9e. Remaining 4 issues are for missing E2E tests (sidebar_tui-yv7, sidebar_tui-mpt, sidebar_tui-aur, sidebar_tui-c20).

## 2026-02-16 - Fixed Spec/Implementation Mismatch for Selection Color

Fixed sidebar_tui-zoz: Updated objectives.md line 67 to reflect that the selected session background uses dark grey (color 238) instead of dark purple (color 54). The implementation was changed per user request but the spec was never updated. All 365 lib tests pass. Remaining 5 issues are for missing E2E tests.

## 2026-02-16 - Verification Check Found Incomplete Work

Verification check found incomplete work:
1. **Spec/Implementation mismatch**: objectives.md line 67 specifies "dark purple (color 54)" for selected session background, but implementation uses DARK_GREY (238) per user request. The spec was NOT updated to reflect this change (sidebar_tui-zoz).
2. **Missing E2E tests per spec requirement** (line 20: "at least one E2E test for each individual bullet point"):
   - Session name wrapping with `│` and `└` characters (sidebar_tui-yv7)
   - Truncation indicators when session list overflows (sidebar_tui-mpt)
   - Space key and right arrow for focusing terminal from sidebar (sidebar_tui-aur)
   - Vim j/k navigation keys in sidebar (sidebar_tui-s9e)
   - Esc "Jump Back" feature from sidebar (sidebar_tui-c20)

## 2026-02-16 - Added E2E Test for Esc Jump Back Feature

Completed sidebar_tui-c20: Added E2E test `test_esc_jump_back` to verify that pressing Esc in the sidebar performs "Jump Back" - returning focus to the terminal AND restoring selection to the session that was selected before the sidebar was focused. The test creates two sessions, focuses sidebar, navigates to a different session with 'j', then presses Esc and verifies: (1) terminal regains focus (sidebar border unfocused), (2) selection returns to the original session. Also fixed flaky `test_quit_confirmation` by adding a polling loop for the confirmation prompt. All 365 lib + 65 bin + 37 E2E tests pass. Binary reinstalled. Closed sidebar_tui-c20. Remaining 3 issues are for missing E2E tests (sidebar_tui-yv7, sidebar_tui-mpt, sidebar_tui-aur).

## 2026-02-16 - Added E2E Tests for Space and Right Arrow Keys

Completed sidebar_tui-aur: Added two E2E tests `test_space_focuses_terminal_from_sidebar` and `test_right_arrow_focuses_terminal_from_sidebar` to verify that Space and Right Arrow keys work as alternative ways to focus the terminal from the sidebar (per spec: "enter, space, or → - Select: Focus on the terminal pane"). Also added `send_space()` and `send_right_arrow()` helper methods to SbSession. All 365 lib + 65 bin + 39 E2E tests pass. Binary reinstalled. Closed sidebar_tui-aur. Remaining 2 issues are for missing E2E tests (sidebar_tui-yv7, sidebar_tui-mpt).

## 2026-02-16 - Added E2E Test for Session Name Wrapping with Continuation Indicators

Completed sidebar_tui-yv7: Added E2E test `test_session_name_wrapping_with_continuation_indicators` to verify that long session names wrap correctly with `│` (middle) and `└` (end) continuation indicators. The test creates a 50-character session name (exceeding the 24-char content width) and verifies: (1) the name wraps to multiple lines, (2) continuation indicators are present, (3) indicators are colored dark grey (238) per spec. Also fixed flaky `test_focus_switching` by adding polling loops instead of fixed sleeps and `cleanup_test_sessions()` call. All 365 lib + 65 bin + 40 E2E tests pass. Binary reinstalled. Closed sidebar_tui-yv7. Remaining 1 issue for missing E2E test (sidebar_tui-mpt: truncation indicators).

## 2026-02-16 - Added E2E Test for Truncation Indicators When Session List Overflows

Completed sidebar_tui-mpt: Added E2E test `test_truncation_indicators_when_session_list_overflows` to verify that truncation indicators (`...`) appear when more sessions exist than can fit in the visible sidebar area. The test creates 25 sessions programmatically (more than the ~17 visible rows), then verifies: (1) the truncation indicator `...` appears in the sidebar, (2) the indicator is colored dark grey (238) per spec. This completes the missing E2E test coverage per spec lines 68-70. All 365 lib + 65 bin + 41 E2E tests pass. Binary reinstalled. Closed sidebar_tui-mpt. No remaining issues.

## 2026-02-17 - Implemented Workspace Data Model and Sidebar Header

Fixed failing `test_vi_editing_workflow` E2E test (stale vim swap file causing ATTENTION dialog). Implemented complete workspace data model in daemon.rs: `WorkspaceMetadata` struct with persistence, `workspace_name` field added to `SessionInfo` and `SessionMetadata`, workspace CRUD operations in `ClientMessage`/`DaemonResponse`, and all process_message handlers for ListWorkspaces, CreateWorkspace, RenameWorkspace, DeleteWorkspace, SwitchWorkspace, MoveSessionToWorkspace, SaveWorkspaceState. Daemon auto-creates "Default" workspace on first run. Added `DaemonClient` workspace methods. Updated sidebar to show active workspace name (not "Sidebar TUI"). Updated AppState with `workspace_name` field. Updated `run_attached` to load workspace info and filter sessions by active workspace. Added 9 new workspace unit tests (369 lib + 41 E2E all pass). Closed sidebar_tui-hka and sidebar_tui-56v.

## 2026-02-18 - Added Missing E2E Tests for Spec Coverage

Audited spec vs E2E test coverage and found 5 bullet-point requirements without dedicated tests. Added 5 new E2E tests: `test_ctrl_n_from_terminal_enters_create_mode` (mod+n from terminal pane enters create mode), `test_ctrl_w_from_terminal_opens_workspace_overlay` (mod+w from terminal opens workspace overlay), `test_delete_session_focus_transitions` (deleting a session moves focus to next/previous session per spec), `test_session_name_character_restrictions` (invalid chars like !, @, # are rejected in session rename), and `test_workspace_name_truncated_in_sidebar_header` (workspace names > 24 chars show ... truncation in sidebar). All 380 lib + all E2E tests pass. Closed sidebar_tui-cze, sidebar_tui-1ub, sidebar_tui-979, sidebar_tui-kze, sidebar_tui-uf6.
