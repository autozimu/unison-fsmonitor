# unison-fsmonitor

[![Github Actions Status](https://github.com/autozimu/unison-fsmonitor/workflows/build-and-test/badge.svg)](https://github.com/autozimu/unison-fsmonitor/actions?query=workflow%3Abuild-and-test)

A cross-platform file system monitor for [Unison](https://github.com/bcpierce00/unison) that enables the `-repeat watch` option.

## Why

Unison doesn't include `unison-fsmonitor` for macOS, thus the `-repeat watch` option doesn't work out of the box. This utility fills the gap. While originally made for macOS, it works on other platforms as well.

## Status

**Stable and Production Ready** - This project has been used in production for many years by the author and colleagues to sync codebases between macOS and Linux. It is actively maintained with regular releases.

## Supported Platforms

This project uses the [notify](https://github.com/notify-rs/notify) crate for cross-platform file system monitoring:

| Platform | Status | Kernel Interface |
|----------|--------|------------------|
| macOS (Intel & Apple Silicon) | ✅ Tested | FSEvents |
| Linux | ✅ Tested | inotify |
| Windows | ✅ Tested | ReadDirectoryChangesW |
| BSDs (FreeBSD, NetBSD, OpenBSD) | ⚠️ Theoretically supported* | kqueue |
| illumos/Solaris | ⚠️ Theoretically supported* | - |

*BSDs and illumos are theoretically supported via the notify crate but have not been tested by the maintainer. Contributions for testing on these platforms are welcome!

For a complete list of platforms supported by the notify crate, see: https://github.com/notify-rs/notify#platforms

## Requirements

### For Installation (via Homebrew or Cargo)
- No additional requirements beyond what Homebrew or Cargo provides.

### For Building from Source
- **Rust compiler** (stable toolchain) - Install via [rustup](https://rustup.rs/)
- Platform-specific requirements:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Linux**: Standard build tools (gcc, make, etc.) - usually pre-installed
  - **Windows**: Microsoft Visual C++ Build Tools or equivalent

## Install

### Via Homebrew (macOS)

```sh
brew install autozimu/homebrew-formulas/unison-fsmonitor
```

### Via Cargo (all platforms)

If you have [Rust and Cargo](https://rustup.rs/) installed:

```sh
cargo install unison-fsmonitor
```

### From Source

```sh
git clone https://github.com/autozimu/unison-fsmonitor.git
cd unison-fsmonitor
cargo build --release
# Binary will be at target/release/unison-fsmonitor. Link/Add to your PATH.
```

## Usage

Simply run unison with `-repeat watch` as argument or `repeat=watch` in your config file:

```sh
unison -repeat watch /path/to/replica1 /path/to/replica2
```

Or in your Unison profile (`~/.unison/*.prf`):
```
repeat = watch
```

## File Watch Limits

You might need to update file watch limits on both hosts if the watching limit is reached. See the [Watchman documentation](https://facebook.github.io/watchman/docs/install#system-specific-preparation) for system-specific instructions.

## Debug

To enable debug logging:

```sh
RUST_LOG=debug unison
```

## References

- Unison fsmonitor protocol: <https://github.com/bcpierce00/unison/blob/af8669bb26f88e85bdc37cb1ff23d9bb0685a1e2/src/fswatch.ml>
- Unison watcher common: <https://github.com/bcpierce00/unison/blob/master/src/fsmonitor/watchercommon.ml>
- Unox (another fsmonitor implementation): <https://github.com/hnsl/unox>
- Notify crate (underlying file watching library): <https://github.com/notify-rs/notify>
