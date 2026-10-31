# CalmIG

CalmIG is a Rust/Tauri desktop app derived from instagram-cli. See README.md for setup and commands.

- Keep the calendar chronological and limited to followed accounts; exclude videos, stories, Reels, music and recommendations.
- Treat sessions, cookies, private posts and messages as sensitive. Never log or commit them; use fixtures and mocks in tests.
- Check both Russian and English interface text and preserve user-generated content when changing localization.
- Run Rust core and desktop tests and formatting checks, plus `npm test` for interface changes. For desktop changes also run `npm run build`. The build needs the Tauri Linux system dependencies listed in README.md.
- Preserve upstream attribution and the MIT license.
