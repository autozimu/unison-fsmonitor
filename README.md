# unison-fsmonitor

[![Github Actions Status](https://github.com/autozimu/unison-fsmonitor/workflows/build-and-test/badge.svg)](https://github.com/autozimu/unison-fsmonitor/actions?query=workflow%3Abuild-and-test)

## Why

`unison` doesn't include `unison-fsmonitor` for macOS, thus `-repeat watch` option doesn't work out of the box. This utility fills the gap. This implementation was originally made for macOS but shall work on other platforms as well like Linux, Windows.

## Install

```sh
brew install autozimu/homebrew-formulas/unison-fsmonitor
```

Alternatively if you have [cargo](https://github.com/rust-lang/cargo) installed,

```sh
cargo install unison-fsmonitor
```

## Usage

Simply run unison with `-repeat watch` as argument or `repeat=watch` in config file.

## File watch limits 

You might need to update file watch limits in both hosts if watching limit reached. See <https://facebook.github.io/watchman/docs/install#system-specific-preparation> for more details.

## Debug

```
RUST_LOG=debug unison
```

## References

- <https://github.com/bcpierce00/unison/blob/master/src/fsmonitor/watchercommon.ml>
- <https://github.com/hnsl/unox>
