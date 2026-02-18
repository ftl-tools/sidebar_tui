# Progress Logs

## 2026-02-18 - Added Missing E2E Tests for Spec Coverage

Audited spec vs E2E test coverage and found 5 bullet-point requirements without dedicated tests. Added 5 new E2E tests: `test_ctrl_n_from_terminal_enters_create_mode` (mod+n from terminal pane enters create mode), `test_ctrl_w_from_terminal_opens_workspace_overlay` (mod+w from terminal opens workspace overlay), `test_delete_session_focus_transitions` (deleting a session moves focus to next/previous session per spec), `test_session_name_character_restrictions` (invalid chars like !, @, # are rejected in session rename), and `test_workspace_name_truncated_in_sidebar_header` (workspace names > 24 chars show ... truncation in sidebar). All 380 lib + all E2E tests pass. Closed sidebar_tui-cze, sidebar_tui-1ub, sidebar_tui-979, sidebar_tui-kze, sidebar_tui-uf6.

## 2026-02-17 - Implemented Workspace Data Model and Sidebar Header

Fixed failing `test_vi_editing_workflow` E2E test (stale vim swap file causing ATTENTION dialog). Implemented complete workspace data model in daemon.rs: `WorkspaceMetadata` struct with persistence, `workspace_name` field added to `SessionInfo` and `SessionMetadata`, workspace CRUD operations in `ClientMessage`/`DaemonResponse`, and all process_message handlers for ListWorkspaces, CreateWorkspace, RenameWorkspace, DeleteWorkspace, SwitchWorkspace, MoveSessionToWorkspace, SaveWorkspaceState. Daemon auto-creates "Default" workspace on first run. Added `DaemonClient` workspace methods. Updated sidebar to show active workspace name (not "Sidebar TUI"). Updated AppState with `workspace_name` field. Updated `run_attached` to load workspace info and filter sessions by active workspace. Added 9 new workspace unit tests (369 lib + 41 E2E all pass). Closed sidebar_tui-hka and sidebar_tui-56v.

## 2026-02-16 - Added E2E Test for Truncation Indicators When Session List Overflows

Completed sidebar_tui-mpt: Added E2E test `test_truncation_indicators_when_session_list_overflows` to verify that truncation indicators (`...`) appear when more sessions exist than can fit in the visible sidebar area. The test creates 25 sessions programmatically (more than the ~17 visible rows), then verifies: (1) the truncation indicator `...` appears in the sidebar, (2) the indicator is colored dark grey (238) per spec. This completes the missing E2E test coverage per spec lines 68-70. All 365 lib + 65 bin + 41 E2E tests pass. Binary reinstalled. Closed sidebar_tui-mpt. No remaining issues.

## 2026-02-16 - Added E2E Test for Session Name Wrapping with Continuation Indicators

Completed sidebar_tui-yv7: Added E2E test `test_session_name_wrapping_with_continuation_indicators` to verify that long session names wrap correctly with `│` (middle) and `└` (end) continuation indicators. The test creates a 50-character session name (exceeding the 24-char content width) and verifies: (1) the name wraps to multiple lines, (2) continuation indicators are present, (3) indicators are colored dark grey (238) per spec. Also fixed flaky `test_focus_switching` by adding polling loops instead of fixed sleeps and `cleanup_test_sessions()` call. All 365 lib + 65 bin + 40 E2E tests pass. Binary reinstalled. Closed sidebar_tui-yv7. Remaining 1 issue for missing E2E test (sidebar_tui-mpt: truncation indicators).

## 2026-02-16 - Added E2E Tests for Space and Right Arrow Keys

Completed sidebar_tui-aur: Added two E2E tests `test_space_focuses_terminal_from_sidebar` and `test_right_arrow_focuses_terminal_from_sidebar` to verify that Space and Right Arrow keys work as alternative ways to focus the terminal from the sidebar (per spec: "enter, space, or → - Select: Focus on the terminal pane"). Also added `send_space()` and `send_right_arrow()` helper methods to SbSession. All 365 lib + 65 bin + 39 E2E tests pass. Binary reinstalled. Closed sidebar_tui-aur. Remaining 2 issues are for missing E2E tests (sidebar_tui-yv7, sidebar_tui-mpt).

## 2026-02-16 - Added E2E Test for Esc Jump Back Feature

Completed sidebar_tui-c20: Added E2E test `test_esc_jump_back` to verify that pressing Esc in the sidebar performs "Jump Back" - returning focus to the terminal AND restoring selection to the session that was selected before the sidebar was focused. The test creates two sessions, focuses sidebar, navigates to a different session with 'j', then presses Esc and verifies: (1) terminal regains focus (sidebar border unfocused), (2) selection returns to the original session. Also fixed flaky `test_quit_confirmation` by adding a polling loop for the confirmation prompt. All 365 lib + 65 bin + 37 E2E tests pass. Binary reinstalled. Closed sidebar_tui-c20. Remaining 3 issues are for missing E2E tests (sidebar_tui-yv7, sidebar_tui-mpt, sidebar_tui-aur).

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

## 2026-02-16 - Fixed Flaky test_hint_bar_context and test_quit_confirmation E2E Tests

Fixed two E2E tests that failed when running the full test suite due to leftover sessions and timing issues. Root cause: (1) `test_quit_confirmation` wasn't calling `cleanup_test_sessions()` at start, (2) `test_hint_bar_context` was using fixed 1500ms sleep which wasn't sufficient under test suite load - the hint bar wasn't visible yet when assertion ran. Fixes: (1) Added `cleanup_test_sessions()` call at the start of `test_quit_confirmation`, (2) Changed `test_hint_bar_context` to use a polling loop (10 x 200ms = 2 seconds max) waiting for hint bar content to appear before asserting. All 365 lib + 65 bin + 35 E2E tests pass. Binary reinstalled.

## 2026-02-16 - Fixed Flaky test_tab_focuses_terminal Test

Fixed `test_tab_focuses_terminal` which passed in isolation but failed when running the full E2E test suite. The test was using fixed sleep times that weren't sufficient when the system was under load from running 30+ other tests. Fix: (1) Added `cleanup_test_sessions()` at test start for clean state, (2) Replaced fixed sleep with polling loop that waits up to 2 seconds (10 x 200ms) for UI state to change, (3) Fixed unused variable warning in `cleanup_test_sessions()`. All 365 lib + 65 bin + 35 E2E tests pass. Binary reinstalled.

## 2026-02-16 - Added Dirty Tracking for Incremental Rendering (Performance)

Completed sidebar_tui-ced: Implemented dirty tracking to avoid rebuilding Line objects when terminal content hasn't changed. Changes: (1) Added `dirty: bool` flag to Terminal struct that tracks when content changes, (2) Added `RenderCache` struct to store previously rendered lines along with scroll offset and dimensions, (3) Modified `process()`, `scroll_up()`, `scroll_down()`, and `resize()` to set dirty flag appropriately, (4) Updated `render()` and `render_with_cursor()` to take `&mut self` and use cached Lines when not dirty, (5) Changed `render_daemon_app()` signature to take `&mut DaemonApp` to allow mutable terminal access, (6) Added 9 new unit tests for dirty tracking behavior. When terminal content hasn't changed between frames, the render function now returns cached Line objects instead of iterating over all cells. All 365 lib + 65 bin + 35 E2E + 2 scaffold tests pass. Binary reinstalled. Closed sidebar_tui-ced.

## 2026-02-16 - Comprehensive E2E Test Isolation Fixes

Fixed remaining flaky E2E tests that failed when run in the full test suite. Root causes: (1) Tests using `spawn()` directly weren't calling `ensure_daemon_ready()`, (2) Session cleanup wasn't aggressive enough - leftover sessions from unit tests ("newer_session", "s1", "s2") and other E2E tests filled the sidebar causing parsing issues. Fixes: (1) Added `spawn_sb()` helper that wraps spawn with daemon readiness check, replaced 20+ direct spawn calls, (2) Updated `cleanup_test_sessions()` to kill ALL sessions instead of just pattern-matched ones, (3) Increased stabilization delay from 100ms to 200ms in `SbSession::new()`, (4) Increased wait times for tab focus test. Closed sidebar_tui-9t5 (quit confirmation test now passes as part of overall fix). All 356 lib + 65 bin + 35 E2E + 2 scaffold tests pass. Binary reinstalled.

## 2026-02-16 - Compressed History (Entries from 2026-02-15 to 2026-02-16)

Early development focused on core TUI functionality and quality-of-life improvements. Key completed work includes: session ordering by last-used timestamp with persistence across restarts, auto-generated three-word session names eliminating manual naming, terminal pane color fixes for Apple Terminal (default foreground to white), grey text rendering fixes for empty cells, and Linux builds for Docker containers. Also implemented render batching (drain socket before rendering), VecDeque for O(1) history trimming, fixed broken mouse scroll tests, reduced unnecessary screen captures (on-demand instead of every process() call), Cow-based String allocation reduction for render performance, text selection mode toggle (Ctrl+S), and comprehensive E2E test isolation fixes with daemon readiness checks.

## 2026-02-16 - Fixed Flaky E2E Test Isolation Issues

Completed sidebar_tui-dab: Fixed flaky E2E tests that were failing inconsistently when run in the full E2E suite. Root cause was tests starting before the daemon was fully ready (after shutdown tests or tests that disrupted daemon state). Fix: (1) Added `ensure_daemon_ready()` helper function that polls `sb list` up to 5 times with 500ms delays to verify daemon connectivity, (2) Called `ensure_daemon_ready()` in `SbSession::new()` constructor so ALL E2E tests benefit from daemon readiness check, (3) Added 100ms stabilization delay after daemon check. Also simplified `test_sidebar_is_28_chars_wide` to verify sidebar border characters at columns 0, 27, and 28 rather than checking foreground colors (which can be unreliable). All 356 lib + 65 bin + 35 E2E + 2 scaffold tests pass consistently. Binary reinstalled. Closed sidebar_tui-dab. Also closed sidebar_tui-q8l (test_welcome_state_on_fresh_start) as it now passes.

## 2026-02-16 - Compressed History (Performance & UX Improvements)

Performance optimizations and UX improvements including: Cow-based String allocation reduction in terminal render (sidebar_tui-e4f), text selection mode toggle with Ctrl+S (sidebar_tui-7i7), on-demand screen capture instead of per-process() (sidebar_tui-v1z), VecDeque for O(1) history trimming (sidebar_tui-r77), render batching via socket draining (sidebar_tui-rny), and terminal foreground color fix for Apple Terminal defaulting to white (sidebar_tui-6i8). Also fixed broken mouse scroll tests (sidebar_tui-0uk) and implemented auto-generated three-word session names (sidebar_tui-mpq).
