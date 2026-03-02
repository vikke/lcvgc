# Live CV Gate Coder

## What is this?

A live coding engine that describes MIDI sequences in a text-based DSL and evaluates/plays them in real time.
A live coding toolkit supporting everything from modular synths (MIDI to CV) to general MIDI synths.

## Installation

### Requirements

- Rust (edition 2021)
- Linux: ALSA development library
  - Debian/Ubuntu: `sudo apt install libasound2-dev`
  - Fedora/RHEL: `sudo dnf install alsa-lib-devel`

### Install

```sh
cargo install --git https://github.com/vikke/lcvgc lcvgc
cargo install --git https://github.com/vikke/lcvgc lcvgc-lsp
```

## Related Projects

- [lcvgc.nvim](https://github.com/vikke/lcvgc.nvim) — Neovim plugin

## Specifications

- [DSL Specification (English)](specs/lcvgc-dsl-spec.md)
- [DSL 仕様書（日本語）](specs/lcvgc-dsl-spec.ja.md)
