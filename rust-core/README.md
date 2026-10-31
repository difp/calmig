# CalmIG Rust core — experimental

The Rust core handles browser-session preparation, compatible legacy device
identities, chronological calendar synchronization, profiles, comments and
Direct messages. The desktop app uses Rust for these operations and for
compatible saved sessions. The application no longer supports the legacy password-login adapter. Browser cookies and tokens
are never returned to the interface or printed in diagnostics.

The compatible legacy device generator derives from Chance.js (MIT); its
license is included in [CHANCE-LICENSE](CHANCE-LICENSE).

Run `cargo test --manifest-path rust-core/Cargo.toml` to check the core with
synthetic sessions and local fake servers. For a read-only diagnostic against
Instagram using an existing legacy session, explicitly provide its path:

```sh
cargo run --manifest-path rust-core/Cargo.toml --bin calmig-probe -- /path/to/session.ts.json
```

This sends one authenticated request and prints only the number of followed
accounts in the first page and whether another page exists. It does not print
session data, account names or response bodies. Do not share session files.
Comment and message sending have only been checked against local fake servers.
Instagram's unofficial API may change or restrict access.
