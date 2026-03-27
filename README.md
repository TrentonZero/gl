# gl

`gl` is a terminal UI for browsing local git branches as branch-level diffs.

Today the app is focused on one core workflow:

- show local branches
- group Graphite stacks when `gt` is available
- open a branch as a combined diff against its base
- navigate the diff quickly in the terminal

## Current Features

- Local branch list with:
  - current branch indicator
  - ahead/behind tracking status
  - async commit-count enrichment
  - Graphite stack grouping and stale-branch markers when `gt` is available
- Branch detail view with:
  - combined branch diff against upstream, detected trunk, or Graphite parent
  - syntax-highlighted diff rendering via `syntect`
  - file-header jump navigation
  - in-diff search with `n` and `N`
  - unified or side-by-side layout toggled with `v`
  - whitespace-insensitive reload toggled with `w`
  - background preload of branch diffs and highlighted output after first paint
- Debounced automatic refresh when repository files, refs, `HEAD`, or the index change
- Status view for the checked-out branch with:
  - working tree summary counts for staged, unstaged, and untracked files
  - staged and unstaged combined diffs in the existing review pane
  - untracked-file listing in the same jumpable file roster
- Stack view with:
  - selected-branch parent/child/base summary
  - ordered stack branch roster with stale and tracking indicators
- Graph view with:
  - first-parent local commit history for all local branches
  - branch-head labels in the graph list
  - `Enter` to open the owning branch in branch detail
- Manual refresh with `R` as a fallback when filesystem watching is unavailable
- Optional top and bottom chrome via `~/.config/gl/config.toml`
- Lightweight profiling logs when `GL_PROFILE=1`

## What Is Not Implemented Yet

The repository still contains broader design docs for worktrees, graph view, command mode, and richer config. Those are not implemented in the current binary.

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.70+)
- Git

## Building

```sh
cargo build --release
```

The compiled binary will be at `target/release/gl`.

## Installation

Copy the binary somewhere on your PATH:

```sh
cp target/release/gl /usr/local/bin/
```

Or install directly via Cargo:

```sh
cargo install --path .
```

## Usage

```sh
# Run in the current directory
gl

# Run against a specific repo
gl /path/to/repo
```

## Keybindings

Branch list:

- `j` / `k`: move selection
- `J` / `K`: jump between stack groups
- `gg` / `G`: jump to first or last branch
- `Ctrl-d` / `Ctrl-u`: move faster through the list
- `4`: open or close graph view
- `Enter`: open selected branch diff
- `S`: open the checked-out branch's working tree status
- `s`: open or close the selected branch's stack view
- `R`: refresh repository data
- `?`: show help
- `q`: quit

Branch detail:

- `Tab`: switch focus between branch list and diff pane
- `Esc`: close the detail view

Graph view:

- `j` / `k`: move selection
- `J` / `K`: jump between branch heads
- `gg` / `G`: top or bottom
- `Ctrl-d` / `Ctrl-u`: move faster through the graph
- `Tab`: switch focus back to branch list
- `Enter`: open the selected commit's owning branch

Diff pane:

- `j` / `k`: scroll
- `J` / `K`: jump between file headers
- `gg` / `G`: top or bottom
- `Ctrl-d` / `Ctrl-u`: half-page scroll
- `v`: toggle unified / side-by-side diff view
- `w`: ignore or show whitespace-only changes
- `/`: start search
- `n` / `N`: next or previous match

## Config

Config path:

```sh
~/.config/gl/config.toml
```

Current supported options:

```toml
chrome = true
diff_view = "unified"
ignore_whitespace = false
```

Set `chrome = false` to hide the top status bar and bottom help bar. Set `diff_view = "side-by-side"` or `ignore_whitespace = true` to start in those diff modes.

## Profiling

Set `GL_PROFILE=1` to emit simple timing logs to stderr for startup, refresh, stack detection, diff loading, and syntax highlighting.
