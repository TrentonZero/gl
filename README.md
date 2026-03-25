# gl

A terminal UI for browsing git diffs by branch, with first-class support for Graphite-style stacks.
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

## Current Features

- Flat or stack-grouped local branch list with ahead/behind, commit count, and stale indicators
- Branch detail diff view with file jumping, search, and syntax highlighting
- Stack View for the selected Graphite stack via `s` or `2`
- Graceful degraded mode when `gt` is missing or the repo is not Graphite-initialized

## Keybindings

- Branch list: `j`/`k` move, `J`/`K` jump stack groups, `Enter` open branch diff, `s` open Stack View
- Global: `2` open Stack View for the selected branch's stack, `R` refresh, `?` help, `q` quit
- Diff view: `j`/`k` scroll, `J`/`K` jump files, `gg`/`G` ends, `/` search
- Stack View: `j`/`k` move, `gg`/`G` ends, `Enter` open selected branch diff, `Esc` return
