# unison-fsmonitor

## Why
`unison` doesn't include `unison-fsmonitor` for macOS, thus `-repeat watch` option doesn't work out of the box. This utility fills the gap.

## Usage
```
cargo build --release
ln -s $PWD/target/release/unison-fsmonitor /usr/local/bin/
```

## Debug
```
cargo build
ln -s $PWD/target/debug/unison-fsmonitor /usr/local/bin/
RUST_LOG=debug unison
```

## Credit
- <https://github.com/hnsl/unox>
