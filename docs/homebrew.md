# Homebrew Packaging

`gl` is a terminal binary, so Homebrew should package it as a formula rather than a cask.

## Local Install

From the application repository root, tap the sibling Homebrew repository and install from it:

```sh
brew tap TrentonZero/gl /Users/kwalker/git/homebrew-gl
brew install --HEAD TrentonZero/gl/gl
```

The current `head` source points at the public GitHub repository for `gl`.

To reinstall after formula changes:

```sh
brew reinstall --HEAD TrentonZero/gl/gl
```

## Publishing As A Tap

The conventional tap layout is a separate repository named `homebrew-gl` with the formula at `Formula/gl.rb`.

Recommended release flow:

1. Create a GitHub release for a tagged version.
2. Attach a source tarball or platform archive.
3. Replace the `head` stanza with `url` and `sha256`.
4. Set the formula `version` explicitly if Homebrew cannot infer it from the URL.

Example stable stanza:

```ruby
url "https://github.com/TrentonZero/gl/archive/refs/tags/v0.1.0.tar.gz"
sha256 "<fill me in>"
version "0.1.0"
```
