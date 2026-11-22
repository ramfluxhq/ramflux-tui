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

The client pins SDK revision `78918c088a487a390dedc60bb04e033ca0120e76`.
Approval rows use the SDK approval list and grant decisions. The message panel reads native cards with `surface.list` and `surface.get`.
Press `c` to open cards in the selected conversation, `[` / `]` to switch cards,
Up/Down to select a control, Enter to edit or act, and Esc to return. Choice inputs
use the displayed option numbers; separate multiple selections with commas.
Buttons send `surface.action` bound to the shown version, hash and input values.
A submitted action waits for an agent receipt; transport submission does not
establish execution. Grant approve/deny remains in the Approvals panel.

Only cards authorized by the daemon as the user's own agents are interactive.
Other authors retain text fallback. Unsupported components, repeated templates,
display functions and link buttons show a clear placeholder. This terminal does
not open external links. Closed and out-of-sync cards have no active controls.
Card copy follows this client's existing English UI; it has no i18n catalog.

The retired renderer is removed. Obsolete group ban, invite and
group-message tombstone helpers, restored-device activation and manual sync import
are removed as well. Native group invitation support requires a separate client
flow with the SDK's invitation identifiers.

## Licensing

Ramflux TUI is licensed under AGPL-3.0-or-later with the Ramflux App Store
Exception. See `LICENSE` and `LICENSING.md`.

Ramflux is part of Span Brain.

Device revocation requires a daemon acknowledgement containing the matching device,
a successful revocation result and the signed device-set epoch. The contacts panel
shows that epoch after success. Upgrade the daemon and SDK together for root-bound
registration and signed revocation; older acknowledgements do not report success.
