# unison-fsmonitor

## Why
`unison` doesn't include `unison-fsmonitor` for macOS, thus `-repeat watch` option doesn't work out of the box. This utility fills the gap.

## Install
```sh
brew install autozimu/formulas/unison-fsmonitor
```
Alternatively if you have [cargo](https://github.com/rust-lang/cargo) installed,
```sh
cargo install unison-fsmonitor
```

## Usage
Simply run unison with `-repeat watch` as argument or `repeat=watch` in config file.

## Debug
```
RUST_LOG=debug unison
```

## Credit
- <https://github.com/hnsl/unox>
