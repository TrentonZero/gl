# GL — Implementation Plan

**Date:** March 25, 2026

## Current Status

- Phase 1 is complete.
- Phase 2 is complete.
- Phase 3 is complete.
- Phase 4 is complete, including degraded-mode fallback messaging when Graphite metadata is unavailable.
- Phase 5 is complete with a dedicated Stack View opened via `s` or `2`.
- Phases 6 through 12 remain open.

This plan breaks the GL build into phases. Each phase produces a working, testable binary. No phase depends on a later phase, and each one adds visible functionality you can use immediately against a real repository.

---

## Phase 1: Skeleton and Branch List

**Goal:** A running TUI that opens a git repo and shows local branches.

**Tasks:**

1. Initialize the Rust project with Cargo. Add dependencies: `ratatui`, `crossterm`, `tokio`, `gix`.
2. Build the application shell: alternate screen entry/exit, raw mode, panic handler that restores the terminal, basic event loop reading key events.
3. Implement the chrome layer: status bar (top) with app name and repo path, help bar (bottom) with context-sensitive hints. Wire up the `[chrome]` config toggle from `~/.config/gl/config.toml` using a minimal TOML config loader (the `toml` crate).
4. Open the repository at the current working directory using `gix`. Enumerate local branches. For each branch, compute: name, commit count above merge-base with its upstream or tracking branch, ahead/behind remote counts.
5. Render the Branch List view: flat list of branches with j/k navigation, selection highlighting, commit count, ahead/behind glyphs (`↑n`/`↓n`/`✓`), current branch `●` indicator. No stack grouping yet — just a flat alphabetical list.
6. Wire up `q` to quit, `?` to show a help overlay, `R` to refresh branch data.

**Deliverable:** You can launch `gl` in a repo directory and see all local branches with their remote sync status. j/k to navigate, q to quit.

---

## Phase 2: Branch Detail View (Diff)

**Goal:** Select a branch and see its full combined diff.

**Tasks:**

1. When `Enter` is pressed on a branch in the Branch List, compute the merge-base between the branch tip and its base (upstream or parent), then compute `git diff merge-base..tip`.
2. Build a scrollable diff renderer: parse diff output into file headers, hunk headers, context/add/del lines. Color each line type. Render into a Ratatui paragraph or custom widget with vertical scrolling.
3. Implement the split-pane layout: Branch List on the left (narrowed), diff consuming the entire right pane. `Esc` returns to the full-width Branch List.
4. Wire up diff navigation keybindings: j/k scroll, J/K jump between file headers, gg/G top/bottom, Ctrl-d/Ctrl-u half-page.
5. Add `/` search within the diff with `n`/`N` for next/previous match.

**Deliverable:** You can browse branches and instantly see what each branch changes relative to its base, as a single combined diff. This is the core value proposition of GL.

---

## Phase 3: Syntax Highlighting

**Goal:** Diff output is syntax-highlighted by language.

**Tasks:**

1. Add `syntect` as a dependency. Load the default syntax set and a theme (a dark theme that complements the TUI color palette).
2. For each file in the diff, detect the language from the file extension. Apply syntax highlighting to the content of context, add, and del lines. Layer the diff coloring (green/red background tint) on top of the syntax colors.
3. Cache syntax highlighting results per file to avoid recomputing on scroll.

**Deliverable:** Diffs are readable with proper language-aware coloring rather than flat green/red text.

---

## Phase 4: Graphite Stack Integration

**Goal:** Branch List groups branches by stack.

**Tasks:**

1. Detect whether the Graphite CLI (`gt`) is available on PATH. If so, invoke `gt log short` and `gt branch info` to discover stack structure: which branches belong to which stacks, and their ordering.
2. Parse the CLI output into a stack model: a list of stacks, each containing an ordered list of branches (base to tip).
3. Update the Branch List renderer to group branches by stack with visual indentation and connecting border lines. Stack headers show the stack name. Standalone branches appear in a separate section below.
4. Add the `⚠` stale indicator: compare each branch's merge-base with its parent branch to detect divergence.
5. Wire up `J`/`K` in Branch List to jump between stack groups.
6. If `gt` is not available, fall back to flat branch list with a notice.

**Deliverable:** Branch List now shows the stack structure. You can see at a glance which branches are stacked on which, and which are stale.

---

## Phase 5: Stack View

**Goal:** A focused view of a single stack.

**Tasks:**

1. Build the Stack View: vertical list of branches in stack order (base at bottom, tip at top) connected by a dot-and-line graph.
2. For each branch in the stack, display: name, commit count, diff stat summary (files changed, +/- lines), push status (pushed/unpushed), stale indicator.
3. Wire up `s` in Branch List to open Stack View for the selected branch's stack. Wire up `2` globally to switch to Stack View.
4. `Enter` in Stack View opens the Branch Detail diff for the selected branch. `Tab` expands/collapses an inline diff preview.
5. j/k navigation, gg/G for base/tip.

**Deliverable:** You can see an entire stack's health at a glance and drill into any branch.

**Status:** Complete as of March 25, 2026.

---

## Phase 6: Worktree Support

**Goal:** GL is worktree-aware and includes the Worktree Manager.

**Tasks:**

1. On startup, enumerate all worktrees for the repository using `gix` (or `git worktree list --porcelain`). For each worktree, record: path, checked-out branch, clean/dirty status.
2. Determine the active worktree context from the directory GL was launched in.
3. Update the Branch List to show worktree indicator tags on branches that are checked out in a worktree.
4. Build the Worktree Manager view: list of worktrees with path, branch, status. `Enter` to switch GL's active context. `!` to spawn a terminal session in the selected worktree's directory.
5. Wire up `3` globally and `w` in Branch List to open Worktree Manager.

**Deliverable:** GL understands worktrees, shows which branches are checked out where, and lets you switch context between them.

---

## Phase 7: Status View

**Goal:** See staged and unstaged changes for the active worktree.

**Tasks:**

1. For the active worktree, compute `git diff --cached` (staged) and `git diff` (unstaged). Also list untracked files via `git ls-files --others --exclude-standard`.
2. Build the Status View: split pane with staged on top and unstaged on bottom. Each section lists files with status glyph (A/M/D/?) and diff stat.
3. `Enter` expands/collapses inline diff for the selected file. `Tab` switches focus between sections.
4. j/k, J/K, gg/G navigation.
5. Wire up `5` globally to open Status View.

**Deliverable:** You can see what's staged and what's changed in the working tree without leaving GL.

---

## Phase 8: Commit Breakdown

**Goal:** Drill into individual commits within a branch.

**Tasks:**

1. In Branch Detail View, implement the `Tab` toggle: when pressed, overlay a list of commits in the branch (hash, message, timestamp) at the top of the diff pane.
2. Selecting a commit with `Enter` replaces the branch-level diff with that commit's individual diff. `Backspace` returns to the branch-level diff.
3. Build the info overlay (`i` key): transient display of branch metadata — base branch, remote status, worktree path, stack position. Dismisses on any key.

**Deliverable:** You can break a branch down into its constituent commits and inspect each one.

---

## Phase 9: Graph View

**Goal:** A first-parent-only commit graph.

**Tasks:**

1. Walk the first-parent chain from each local branch head. Build a topologically sorted commit list with branch labels.
2. Render a simple ASCII graph column (●, │, ╮, ╯ characters) alongside commit hash, message, and branch label.
3. `Enter` on a commit opens Branch Detail View for the branch that contains it. `e` toggles expansion of a merge commit's merged branch.
4. j/k, J/K (jump to next branch head), gg/G, Ctrl-d/Ctrl-u navigation.
5. Wire up `4` globally.

**Deliverable:** A clean, readable commit graph that stays simple by showing only first-parent history.

---

## Phase 10: Filesystem Watching and Background Refresh

**Goal:** GL updates automatically when the repository changes.

**Tasks:**

1. Add the `notify` crate. Watch `.git/refs/`, `.git/HEAD`, `.git/index`, and worktree directories for changes.
2. Debounce filesystem events (200ms). On change, trigger a background refresh of the affected data: branch list, status, worktree state.
3. Refresh Graphite CLI cache when ref changes are detected.
4. Ensure the UI remains responsive during background refreshes — compute diffs on a background tokio task and send results to the UI thread via a channel.

**Deliverable:** GL stays current without manual `R` refreshes. Change a file in your editor and GL's status view updates.

---

## Phase 11: Side-by-Side Diff and Diff Options

**Goal:** Alternative diff presentation modes.

**Tasks:**

1. Implement side-by-side diff rendering: split the diff pane vertically, old content on the left, new content on the right, with synchronized scrolling.
2. Wire up `v` in Branch Detail View to toggle between unified and side-by-side.
3. Implement `w` to toggle whitespace visibility (pass `--ignore-all-space` to diff computation).
4. Persist the user's preference in config.

**Deliverable:** You can view diffs in the format you prefer.

---

## Phase 12: Command Line, Config, and Polish

**Goal:** Final polish pass.

**Tasks:**

1. Implement the `:` command input line: support `:q` (quit), `:branch <name>` (jump to branch), `:search <term>` (search branches).
2. Implement full config file support: keybinding remapping, color scheme customization, default diff view mode, worktree path defaults.
3. Add command-line arguments: `gl` (open repo at cwd), `gl <path>` (open repo at path), `gl --version`, `gl --help`.
4. Error handling polish: graceful handling of bare repos, repos without remotes, repos without Graphite, corrupted refs.
5. Performance profiling against a large repository (linux kernel scale). Optimize any slow paths identified.

**Deliverable:** GL is feature-complete per the spec, configurable, and handles edge cases gracefully.

---

## Phase Summary

| Phase | What You Get | Key Dependencies |
|-------|-------------|-----------------|
| 1 | Branch list in a TUI | ratatui, crossterm, gix |
| 2 | Combined branch diffs | Phase 1 |
| 3 | Syntax-highlighted diffs | Phase 2, syntect |
| 4 | Stack-grouped branch list | Phase 1, gt CLI |
| 5 | Stack view | Phase 4 |
| 6 | Worktree awareness | Phase 1 |
| 7 | Status view | Phase 6 |
| 8 | Commit breakdown | Phase 2 |
| 9 | Graph view | Phase 1 |
| 10 | Auto-refresh | Phases 1–9, notify |
| 11 | Side-by-side diff | Phase 2 |
| 12 | Config and polish | All phases |

Phases 3–9 are largely independent of each other after Phase 2 and can be reordered based on what you want to use first. The plan above orders them by what provides the most daily value soonest: diffs first, then stacks, then worktrees, then everything else.
