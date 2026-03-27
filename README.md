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
brew tap TrentonZero/gl
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

Config path:

```sh
~/.config/gl/config.toml
```

Current supported options:

```toml
chrome = true
diff_view = "unified"
ignore_whitespace = false
color_scheme = "ocean"
worktree_path_defaults = ["~/src/worktrees"]

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

## Profiling

Set `GL_PROFILE=1` to emit simple timing logs to stderr for startup, refresh, stack detection, diff loading, and syntax highlighting.
