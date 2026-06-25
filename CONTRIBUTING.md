# Contributing

Contributions are welcome after the contributor signs the CLA in `CLA.md`.

## Requirements

- Keep this client under AGPL-3.0-or-later with the Ramflux App Store Exception.
- Sign commits with the Developer Certificate of Origin:

```text
Signed-off-by: Your Name <you@example.com>
```

- Keep new Rust comments and user-facing text in English.
- Add the SPDX header to every Rust source file:

```rust
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain
```

- Do not commit secrets, private endpoints, internal hostnames, or development
  environment paths.

## Local Checks

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
