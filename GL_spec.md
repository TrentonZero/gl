# GL — Green Ledger

## Product Specification

**Version:** 0.1 (Draft)
**Date:** March 18, 2026

---

## Overview

GL is a local-first terminal UI (TUI) for Git, built for stack-based branch workflows. It assumes the user works with Graphite-style stacked branches, uses worktrees as a primary workflow mechanism, and wants to see their local work clearly without noise from the broader remote graph.

The governing philosophy: show me what I have, show me what it looks like as a whole, and let me drill down when I need to. Nothing else. GL is a viewer — it reads your repository state and presents it clearly. All mutations (commits, rebases, pushes, worktree management) happen in the shell.

---

## Core Concepts

### The Branch Is the Unit of Work

GL treats a branch as a single logical change. The primary view of any branch is a unified diff of the entire branch against its base — not a list of individual commits. This is the default, always-visible representation.

Individual commits within a branch are available as an expandable drill-down, but the branch-level diff is the first-class citizen. The mental model: a branch is a patch you're building. Commits are save points within that patch.

### Stacks Are the Organizing Structure

GL understands Graphite-style stacked branches natively. A stack is a directed chain of branches where each branch's base is the previous branch in the stack rather than a long-lived integration branch.

The branch list view should render stacks visually as indented or connected chains, making the dependency structure immediately legible. Branches that are not part of a stack appear as standalone entries.

### First-Parent Only

The commit graph view (when visible) uses first-parent-only traversal by default. Merge commits appear as single nodes; the merged branch's individual commits are hidden unless explicitly expanded. This keeps the history view from becoming an unreadable tangle.

This applies everywhere: the branch diff computation, the log view, and any graph visualization.

Current implementation notes:

- If the Graphite CLI (`gt`) is available and `gt log short --no-interactive` succeeds, GL parses the output into stack groups.
- Stack structure is cached on disk.
- Stale branches are computed asynchronously by comparing each branch-parent merge-base to the parent tip.
- If Graphite data is unavailable, GL shows a non-blocking notice and falls back to inferred local stack relationships when possible.

### Local Only

GL shows only branches that exist locally. There is no "remote branches" browser, no listing of branches that exist only on the remote. If it's not checked out or fetched into a local ref, it doesn't exist in GL's world.

Remote tracking information is displayed only in the context of a local branch — its push status, ahead/behind counts, and remote URL. This information appears as metadata on the local branch, not as a separate navigable entity.

### Worktrees Are First-Class

GL treats `git worktree` as a primary workflow mechanism, not an advanced feature hidden in a menu. The interface should make it trivially easy to:

- See all active worktrees at a glance
- Switch the view context between worktrees
- Understand which worktree corresponds to which branch

Each worktree is a distinct working context. GL should never confuse the state of one worktree with another. Worktree creation and removal happen in the shell; GL detects changes and updates its display.

Current diff behavior:

- Diffs are shown in unified form only.
- File headers include per-file `+added -removed` numstat data when available.
- Diff bodies support syntax highlighting through `syntect`.
- Add and delete lines keep diff tinting on top of syntax colors.
- Binary-file changes are rendered as metadata lines rather than raw content.
- After first paint, GL begins preloading branch diffs and highlighted output into memory in the background.

---

## Views

### 1. Branch List (Primary Navigation)

The main navigation pane. Displays all local branches organized by stack membership.

**Layout:**

- Branches that belong to a stack are grouped and visually connected (indentation, vertical line, or similar affordance)
- Standalone branches (not part of any stack) appear below or in a separate section
- The currently checked-out branch (per worktree) is marked with a `●` glyph
- Each branch entry shows:
  - Branch name
  - Ahead/behind remote counts as `↑n`/`↓n` (if remote tracking branch exists)
  - A `✓` glyph when the branch is fully synced with its remote (ahead 0, behind 0)
  - Number of commits in the branch above its base (e.g., `3c`)
  - Worktree indicator tag (if the branch is checked out in a worktree, showing which one)
  - `⚠` glyph if the branch needs rebase (stale relative to its base)

**Interactions:**

- Navigating to a branch opens its Branch Detail View
- Keybinding to switch to Stack View for the selected branch's stack
- Keybinding to switch to Worktree Manager

### 2. Branch Detail View (Primary Content Area)

The main content area when a branch is selected. Shows the branch as a single unit of work.

**Default state — Unified Branch Diff:**

The entire pane is consumed by the diff. There is no metadata header, file summary bar, or other chrome — just the diff output, scrollable from top to bottom. Files within the diff are separated by file header lines showing the file path and diff stat (e.g., `── src/auth/middleware.rs ── +42 -8`), followed by hunks.

Branch metadata (base branch, remote status, worktree, stack position) is available via a keybinding (`i` for info) as a transient overlay, not as a persistent header.

**Commit Breakdown (Tab toggle):**

Pressing `Tab` reveals individual commits as a collapsible list overlaid at the top of the diff. Selecting a commit filters the diff to show only that commit's changes. Pressing `Tab` again or `Backspace` returns to the full branch diff. This is the only way to access commit-level information; there is no persistent commit panel.

**File navigation:**

The `J`/`K` keybindings jump between file headers within the diff. The `/` keybinding searches within the diff. There is no separate file list panel — navigation is entirely within the diff stream itself.

### 3. Worktree Manager

A dedicated view for seeing worktree state at a glance.

**Displays:**

- List of all active worktrees with their paths, checked-out branches, and status (clean/dirty)
- The bare repository location

**Actions:**

- Switch GL's active context to a different worktree
- Open worktree directory in a new terminal session (for performing mutations there)

### 4. Stack View

A focused view of a single stack's structure and health.

**Displays:**

- Vertical list of branches in stack order (base at bottom, tip at top)
- Each branch shows: name, commit count, diff stat summary (files changed, insertions, deletions), push status
- Visual indicator of which branches need rebase after a base branch was updated

**Actions:**

- Navigate to any branch in the stack to open its Branch Detail View
- Collapse/expand individual branch diffs inline

### 5. Status View

A read-only view of the working tree and staging area for the active worktree. This shows what `git status` and `git diff` would show, split into two sections.

**Layout:**

The view is divided into two scrollable regions, top and bottom:

- **Staged changes** (top) — files in the index that differ from HEAD. Each file shows its path and diff stat. Expanding a file (or scrolling into it) shows its diff.
- **Unstaged changes** (bottom) — files in the working tree that differ from the index. Same presentation: path, diff stat, expandable diff.

Untracked files appear at the end of the unstaged section, listed by path only (no diff).

**Interactions:**

- `j`/`k` to navigate between files
- `Tab` to switch focus between the staged and unstaged sections
- `Enter` to expand/collapse the diff for the selected file
- `J`/`K` to jump to next/previous file

This is strictly a viewer. GL does not stage, unstage, or discard changes — those operations happen in the shell.

### 6. Graph View (Secondary)

An optional, togglable commit graph view. This is not the primary interface but is available for users who occasionally need it.

**Constraints:**

- First-parent only by default
- Scoped to local branches only
- Merge commits rendered as single nodes
- Toggle to expand merged branches when needed
- No remote-only branches appear

### 4. Stack View

Pressing `s` on a stacked branch opens a two-pane layout:

- left pane: branch list
- right pane: selected branch's stack summary

The stack pane shows:

- selected branch
- parent and child within the stack
- diff base used for branch comparison
- stale state
- ordered stack branches with current-branch, stale, and tracking indicators

---

## Worktree Behavior Details

### Context Awareness

GL maintains awareness of which worktree it is "viewing from." This could be determined by:

- The directory from which GL was launched
- An explicit worktree selector in the UI
- The most recently focused worktree

The active worktree context determines:

- Which branch appears as "current" in the branch list
- What the working-tree status reflects (clean/dirty indicator)
- Where file-open operations point

### Creating Worktrees

GL does not create or remove worktrees. These operations are performed in the shell via `git worktree add` and `git worktree remove`. GL detects worktree changes via filesystem watching and updates its display accordingly.

### Worktree Indicators

Throughout the UI, any branch that is currently checked out in a worktree should display a small indicator (glyph, color, or tag) showing which worktree it occupies. This prevents the confusion of trying to check out a branch that's already active in another worktree.

---

## Stack Detection and Management

### Detection

GL discovers stack structure by invoking the Graphite CLI (`gt`) rather than reading Graphite's internal metadata files directly. Relevant commands include:

- `gt log short` — list branches in stack order
- `gt stack` — display the current stack

GL should prefer deriving stack structure from a single `gt log short` snapshot and cache the result, refreshing when the user triggers a sync or when GL detects branch ref changes via filesystem watching. The first frame must stay fast: startup work on the critical path should avoid per-branch CLI fan-out and other N-subprocess patterns before the initial paint. This keeps GL decoupled from Graphite's internal storage format and ensures compatibility across Graphite CLI versions.

If the Graphite CLI is not installed or not initialized in the repository, GL should fall back to manual stack definition or infer stacks from merge-base relationships, and display a notice that stack features are degraded.

### Stale Stack Indicators

When a base branch has been updated (e.g., after the base branch's PR is merged and the stack is pulled), GL should detect that downstream branches may need rebasing. This is indicated visually in both the Branch List and Stack View — a glyph or color change on branches whose merge-base with their parent has diverged. GL does not perform the rebase itself; the user handles that in the shell via `gt stack restack` or `git rebase`.

---

## Diff Presentation

### Branch-Level Diff

The branch-level diff computes `git diff $(git merge-base HEAD base)..HEAD` (or equivalent). This shows exactly what the branch adds relative to its base, regardless of how many commits it took to get there.

File-level changes are aggregated: if a file was modified in commits 1 and 3 but not 2, the branch diff shows the net change.

### Commit-Level Diff

When drilling into individual commits, the diff view switches to per-commit mode. Each commit shows only its own changes, not the accumulated branch diff.

### Diff Options

- Side-by-side or unified view (user preference, sticky)
- Whitespace toggle (ignore/show)
- Syntax highlighting
- Word-level diff highlighting within lines
- File filter / search

---

## What GL Does Not Do

- **No remote branch browsing.** If it's not local, it doesn't exist.
- **No octopus merge visualization.** First-parent only. The tangled graph is someone else's problem.
- **No staging or committing.** GL is a viewer. All mutations happen in the shell.
- **No built-in merge/PR workflow.** Push and PR creation are handled via the Graphite CLI or other tools in the shell.
- **No interactive rebase.** Commit reordering, squashing, and fixup happen in the shell.
- **No stack mutation.** Restacking, branch creation, and branch deletion happen via `gt` or `git` in the shell. GL shows you the state; you act on it elsewhere.
- **No repository cloning or initialization.** GL opens existing repositories.
- **No credential management.** GL delegates authentication to the system's git credential helpers.
- **No multi-repo.** GL is always scoped to a single repository. Launch a separate instance for each repo.

---

## Technical Notes

### Language and TUI Framework

GL is a terminal application written in **Rust**, using **Ratatui** as the TUI framework with **crossterm** as the terminal backend. This combination provides:

- Native performance for large repository operations and diff computation
- Excellent async support via **tokio** for background git operations, filesystem watching, and CLI subprocess management
- Cross-platform terminal compatibility (Linux, macOS, Windows) through crossterm
- A mature, actively-maintained widget library in Ratatui with good support for panes, lists, scrollable text, and syntax-highlighted content

### Git Integration

GL uses **gitoxide** (`gix`) as its primary git library for direct repository access. Gitoxide is a pure-Rust git implementation with strong performance characteristics and full worktree support. For operations where gitoxide support is incomplete or where the canonical `git` behavior is preferable, GL falls back to shelling out to the `git` CLI.

Key integration points:

- **gitoxide** for: ref enumeration, diff computation, merge-base calculation, commit traversal, worktree listing, status
- **git CLI** for: operations where exact behavioral parity with upstream git matters (e.g., log formatting, credential-based fetch if refresh is triggered)
- **Graphite CLI (`gt`)** for: read-only stack discovery — stack structure, branch parent/child relationships

### Syntax Highlighting

Diff content is syntax-highlighted using **tree-sitter** parsers or the **syntect** library (Sublime Text syntax definitions). Syntect is the simpler integration path for a TUI diff viewer; tree-sitter offers more precision if semantic highlighting is desired later.

### Filesystem Watching

GL uses **notify** (Rust crate) to watch for ref changes, worktree status changes, and HEAD updates. Events are debounced and trigger background refreshes of the branch list and status indicators without blocking the UI thread.

### Performance Considerations

- Branch diffs should be computed lazily (on selection, not on startup)
- For large repositories, consider caching merge-base computations
- Worktree status polling should be debounced and run in background threads
- The branch list should load fast even with hundreds of local branches
- Graphite CLI calls should be cached and refreshed on explicit sync or detected ref changes, not on every frame

### Configuration

GL should respect:

- `.gitconfig` settings where relevant (diff algorithms, whitespace handling)
- Graphite CLI configuration (GL delegates stack operations to `gt` and inherits its configuration)
- Its own config file at `~/.config/gl/config.toml` for UI preferences: keybindings, diff view mode, color scheme, worktree path defaults

### Chrome Visibility

The status bar (top) and help bar (bottom) can be hidden via configuration:

```toml
[chrome]
status_bar = false
help_bar = false
```

Both default to `true`. When hidden, the full terminal height is available to the active view. The `?` keybinding still toggles a help overlay regardless of this setting.

---

## Keybindings

GL uses vim-style modal keybindings where natural vim commands map cleanly to the interface. It does not attempt full vim emulation — no ex commands, no registers, no macros. Where a vim key has an obvious meaning in context, GL uses it. Where it doesn't, GL defines its own bindings. All keybindings are remappable in `~/.config/gl/config.toml`.

Current runtime behavior:

- Startup loads repository data synchronously.
- Stack enrichment and commit-count calculation happen on background threads.
- Branch diff and syntax-highlight preload begins asynchronously after the first frame is rendered.
- Manual refresh rebuilds repository state and refreshes the active diff if it is still valid.
- Optional profiling output is enabled through `GL_PROFILE`.

### Global (Available in All Views)

| Key | Action |
|-----|--------|
| `q` | Quit GL |
| `?` | Toggle help overlay (shows keybindings for current view) |
| `1` | Switch to Branch List view |
| `2` | Switch to Stack View |
| `3` | Switch to Worktree Manager |
| `4` | Switch to Graph View |
| `5` | Switch to Status View |
| `R` | Refresh all data (re-invoke `gt`, re-read refs, re-check worktree status) |
| `:` | Command input line (for search, goto branch by name) |
| `Esc` | Cancel current action / close overlay / return to previous view |

### Branch List View

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up |
| `J` / `K` | Move to next / previous stack group |
| `Enter` | Open Branch Detail View for selected branch |
| `s` | Open Stack View for the selected branch's stack |
| `w` | Open Worktree Manager |
| `/` | Filter branches by name (incremental search) |
| `gg` | Jump to first branch |
| `G` | Jump to last branch |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |

### Branch Detail View

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll diff down / up |
| `J` / `K` | Jump to next / previous file header in diff |
| `Tab` | Toggle commit breakdown overlay |
| `Enter` | (In commit breakdown) Select commit to view its individual diff |
| `Backspace` | (In commit diff) Return to branch-level diff |
| `i` | Toggle branch metadata info overlay (base, remote, worktree, stack position) |
| `v` | Toggle side-by-side / unified diff view |
| `w` | Toggle whitespace visibility |
| `gg` | Jump to top of diff |
| `G` | Jump to bottom of diff |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `/` | Search within diff |
| `n` / `N` | Next / previous search match |
| `[` / `]` | Navigate to previous / next branch in stack |

### Stack View

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up through branches in the stack |
| `Enter` | Open Branch Detail View for selected branch |
| `Tab` | Expand / collapse inline diff for selected branch |
| `gg` | Jump to stack base |
| `G` | Jump to stack tip |

### Worktree Manager

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up |
| `Enter` | Switch GL's active context to the selected worktree |
| `!` | Open a terminal session in the selected worktree's directory |

### Graph View

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up |
| `J` / `K` | Jump to next / previous branch head |
| `Enter` | Open Branch Detail View for the branch at the selected commit |
| `e` | Toggle expand/collapse of merge commit's merged branch |
| `gg` | Jump to newest commit |
| `G` | Jump to oldest loaded commit |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |

### Status View

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up through files |
| `J` / `K` | Jump to next / previous file |
| `Tab` | Switch focus between staged and unstaged sections |
| `Enter` | Expand / collapse diff for selected file |
| `gg` | Jump to first file |
| `G` | Jump to last file |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |

---

## Design Decisions (Resolved)

These were originally open questions, now settled:

1. **No staging or committing.** GL is a pure viewer. If the need becomes apparent through use, staging may be added later.
2. **No interactive rebase.** Same rationale. The shell handles mutations.
3. **No stack mutation.** GL invokes `gt` only for read-only stack discovery. Restacking, submitting, and branch management happen in the shell.
4. **Single repository only.** GL is always scoped to one repo. Run multiple instances for multiple repos.
5. **Vim-style keybindings.** Where a vim key has a natural mapping (j/k, gg/G, Ctrl-d/Ctrl-u, /), GL uses it. No additional complexity for the sake of vim completeness.

Current implementation scope:

- Implemented today: branch list, branch-level diff view, syntax highlighting, Graphite stack grouping, stack view, minimal config, and profiling hooks.
- Not yet implemented: worktree manager, status view, commit drill-down, graph view, side-by-side diff, whitespace diff toggles, command mode, and mutation workflows such as commit, stage, rebase, push, or worktree creation.

## Planned Extensions

The most natural next additions, based on the current codebase shape, are:

- a status view using the existing diff rendering path
- richer config and command-line support

Those are roadmap items, not implemented behavior.
