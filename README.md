# gl

A terminal UI for browsing git diffs by branch.

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
