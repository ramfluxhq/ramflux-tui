# Ramflux TUI

Ramflux TUI is the terminal user interface client for Ramflux. It connects to a
local Ramflux daemon through the SDK local bus and provides a keyboard-driven
view of messages, contacts, groups, safety status, and pending approvals.

This repository contains only the AGPL client. The Ramflux core SDK and protocol
crates live in `ramfluxhq/ramflux` and are consumed as Git dependencies.

## Build

```sh
cargo build
```

## Test

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Run

Start or connect to a local Ramflux daemon, then run:

```sh
cargo run --bin rf-tui -- --account <account-id>
```

Use the keyboard to switch panels, inspect conversations, and review pending
approval requests. Operations that require App-side signing remain delegated to
the App approval path.

## Licensing

Ramflux TUI is licensed under AGPL-3.0-or-later with the Ramflux App Store
Exception. See `LICENSE` and `LICENSING.md`.

Ramflux is part of Span Brain.
