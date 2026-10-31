# CalmIG

CalmIG is a Rust/Tauri desktop app with a local Node.js adapter derived from instagram-cli. See README.md for setup and commands.

- Keep the calendar chronological and limited to followed accounts; exclude videos, stories, Reels, music and recommendations.
- Treat sessions, cookies, private posts and messages as sensitive. Never log or commit them; use fixtures and mocks in tests.
- Check both Russian and English interface text and preserve user-generated content when changing localization.
- Run `npm test`; for desktop changes also run `cargo fmt --manifest-path quiet-desktop/src-tauri/Cargo.toml -- --check` and `npm run build`. The build needs the Tauri Linux system dependencies listed in README.md.
- Preserve upstream attribution and the MIT license.
