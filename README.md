# gl

`gl` is an opinionated terminal UI for browsing local git branches as branch-level diffs.

Some things it does (or will do soon enough):
- view local standalone branches
- view graphite branches as stacks
- view worktree states
- view diffs either at the commit level of the branch level (and with or withot whitespace)
- view staged and unstaged diffs

Some things it does not do and probably will never do:
- create/edit/update/push anything
- view remote anything (other than some basic stats for local branches)

A more detailed explanation of the opinions is in [the spec](GL_spec.md). 

This is entirely, 100% vibe coded. That's not how I normally work, but its how this was built.

It's really just meant to be a dashboard to help me keep track of what agents are doing in each work tree and on each branch stack. 

If you find that useful, feel free to check it out. If you don't, that's fine too.

If something is broken, feel free to report an issue, but I don't promise any great level of responsiveness: this is a vibe project, which makes it one level lower on my priority list than a hobby project.

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
- Worktree manager with:
  - clean/dirty worktree status and checked-out branch
  - branch-list worktree tags
  - active-context switching with `Enter`
  - asynchronous metadata loading after the first frame so startup does not wait on worktree scans
- Command overlay with `:q`, `:branch <name>`, and `:search <term>`
- Manual refresh with `R` as a fallback when filesystem watching is unavailable
- Optional top and bottom chrome via `~/.config/gl/config.toml`
- File-backed application logs for normal runs, with optional profiling when `GL_PROFILE=1`

## What Is Not Implemented Yet

The repository still contains broader design docs for mutation workflows such as commit, rebase, push, and worktree creation. Those are not implemented in the current binary.



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

Or install from the sibling Homebrew tap repository:

```sh
brew tap TrentonZero/gl /Users/kwalker/git/homebrew-gl
brew install --HEAD TrentonZero/gl/gl
```

The Homebrew formula lives in the tap repository at `../homebrew-gl/Formula/gl.rb`.
The current `--HEAD` install pulls directly from the public GitHub repository.

## Usage

```sh
# Run in the current directory
gl

# Run against a specific repo
gl /path/to/repo

# Show usage
gl --help

# Show version
gl --version

# Override the configured accent color
gl --color-scheme violet
```

## Keybindings

Branch list:

- `j` / `k`: move selection
- `J` / `K`: jump between stack groups
- `gg` / `G`: jump to first or last branch
- `Ctrl-d` / `Ctrl-u`: move faster through the list
- `4`: open or close graph view
- `3` or `w`: open or close the worktree manager
- `:`: open command overlay
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

Worktree manager:

- `j` / `k`: move selection
- `gg` / `G`: top or bottom
- `Tab`: switch focus back to branch list
- `Enter`: switch GL's active context to the selected worktree

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

Config path precedence:

```sh
$XDG_CONFIG_HOME/gl/config.toml
~/.config/gl/config.toml
```

Current supported options:

```toml
chrome = true
diff_view = "unified"
ignore_whitespace = false
color_scheme = "ocean"

[keybindings]
quit = "q"
help = "?"
refresh = "R"
command = ":"
stack_view = "s"
status_view = "S"
graph_view = "4"
```

Set `chrome = false` to hide the top status bar and bottom help bar. Set `diff_view = "side-by-side"` or `ignore_whitespace = true` to start in those diff modes. `color_scheme` supports `ocean`, `forest`, `amber`, `violet`, `rose`, and `teal` for the accent color, `gl --color-scheme <scheme>` overrides that value for a single launch, and `[keybindings]` lets you remap the supported global shortcuts.

## Logging

GL now writes logs to `GL_LOG_PATH` when set, otherwise to `$XDG_STATE_HOME/gl/gl.log`, then `~/.local/state/gl/gl.log`, and finally the system temp directory as a fallback.

Set `GL_PROFILE=1` to include timing logs for startup, refresh, stack detection, diff loading, and syntax highlighting in that same log file. Set `GL_LOG_STDERR=1` if you also want logs mirrored to stderr.
