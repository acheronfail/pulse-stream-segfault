# `pulse-stream-segfault`

A segfault occurs when calling `stream.set_write_callback(None)` from within the stream's write callback.

This repository showcases this segfault, using two different main loop implementations:

* the threaded main loop provided by libpulse
* the [tokio main loop](https://github.com/danieldg/pulse-binding-rust/tree/b0b8a0b71566e00161fc63a90e7be73028503fbe/pulse-tokio) provided by `libpulse_tokio`

Created for this issue: https://github.com/jnqnfe/pulse-binding-rust/issues/56

## How to run

There are two implementations (threaded and tokio), and two ways to run it (calling `stream.set_write_callback(None)` and not calling it); thus, there are 4 ways to run this example:

```bash
# USAGE:
#   cargo run --bin <impl> -- [segfault]

# threaded, NOT calling `stream.set_write_callback(None)` (works)
cargo run --bin threaded

# threaded, calling `stream.set_write_callback(None)` (segfaults)
cargo run --bin threaded -- segfault

# tokio, NOT calling `stream.set_write_callback(None)` (works)
cargo run --bin tokio

# tokio, calling `stream.set_write_callback(None)` (segfaults)
cargo run --bin tokio -- segfauly
```