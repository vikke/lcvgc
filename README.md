# Live CV Gate Coder

## What is this?

A live coding engine that describes MIDI sequences in a text-based DSL and evaluates/plays them in real time.
A live coding toolkit supporting sequences for modular synths (MIDI to CV) through to general MIDI synths.

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

lcvgc works in conjunction with the following projects:

- [lcvgc.nvim](https://github.com/vikke/lcvgc.nvim) — Neovim plugin. A frontend for connecting to the lcvgc engine, evaluating, and playing back sequences directly from the editor
- [lcvgc_mic](https://github.com/vikke/lcvgc_mic) — A CLI tool that detects pitch in real time from microphone input and generates note text in lcvgc DSL format
- [tree-sitter-cvg](https://github.com/vikke/tree-sitter-cvg) — Tree-sitter grammar for the lcvgc DSL (.cvg files). Used for syntax highlighting in lcvgc.nvim

## Specifications

- [DSL 仕様書（日本語）](specs/lcvgc-dsl-spec.ja.md)
- [DSL Specification (English)](specs/lcvgc-dsl-spec.md)
