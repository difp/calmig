# CalmIG 0.1.0-beta.1

A quiet desktop client for viewing photos from followed Instagram accounts and reading messages. CalmIG shows posts by date, groups photos by day and author, and avoids Reels, stories, music and algorithmic recommendations. The interface supports English and Russian.

CalmIG is an experimental Rust/Tauri desktop app with a local Node.js adapter based on [instagram-cli](https://github.com/supreme-gg-gg/instagram-cli). It uses an **unofficial** Instagram API. It is not affiliated with or endorsed by Instagram or Meta.

![CalmIG calendar with demo photographs](docs/screenshots/calendar-en.png)

The screenshots use demonstration data, not a real account. See the [messages view](docs/screenshots/messages-en.png), [mobile layout](docs/screenshots/mobile-en.png), and [photograph credits](docs/screenshots/CREDITS.md).

## Current status

- Linux desktop development build; no packaged installer yet.
- Photos and photo-only carousels, chronological seven-day calendar, profiles, captions and comments.
- Direct messages and sending comments. Instagram may require verification, limit requests or change its API.
- Fetching photos currently checks followed profiles individually. The first synchronization can be slow or incomplete; the app reports errors and incomplete results.
- The local cache contains recent post metadata, captions and image URLs; images are loaded from Instagram. Sign-in data remains on your device. The optional password-based session is stored in a local file without system keyring encryption.

## Build and run on Linux

Prerequisites: Node.js 22 or newer, npm, Rust and Cargo, and [Tauri v2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux), including WebKitGTK 4.1 and development headers. A network connection is needed for the first dependency download.

```sh
npm ci
./quiet-desktop/build.sh
./quiet-desktop/start.sh
```

The build produces a local development executable at `quiet-desktop/tikho` and a bundled Node adapter in `dist/`. It is tied to the checkout and is not a standalone release package. For an existing Cargo cache, set `CALMIG_CARGO_OFFLINE=1` when running the build script.

The built-in browser opens Instagram's sign-in page. After sign-in, the local adapter uses a session to access the account through an unofficial API. Do not publish session files, cookies, tokens, logs containing private data, or personal screenshots. Use the account's **Sign out** action to remove the app's session and cache.

## Verification

```sh
./node_modules/.bin/tsc --noEmit
npx ava --serial tests/quiet-calendar.test.ts tests/quiet-cache.test.ts tests/quiet-following-page.test.ts tests/quiet-photo-details.test.ts
node quiet-desktop/tests/ui.test.cjs
cargo fmt --manifest-path quiet-desktop/src-tauri/Cargo.toml -- --check
```

These tests use fixtures and mocks; they do not confirm live Instagram behavior or publish messages. See [the desktop notes](quiet-desktop/README.md) and [the Following research](docs/instagram-following-research.md) for limitations.

## Русский

CalmIG — экспериментальный настольный клиент для фотографий подписок и переписок. Лента заменена календарём за семь дней: фотографии показаны по датам и авторам, видео и смешанные карусели исключены. Язык интерфейса можно переключить до входа и в профиле.

Для сборки в Linux установите Node.js 22+, Rust и системные зависимости Tauri v2, затем выполните команды из раздела **Build and run on Linux**. Приложение использует неофициальный доступ к Instagram; первая синхронизация может быть долгой или неполной. Сессии и кеш хранятся локально — не добавляйте их в публичный репозиторий.

## Credits and license

CalmIG is a fork of [instagram-cli](https://github.com/supreme-gg-gg/instagram-cli), originally by Jet Chiang and James Zheng. Their [MIT license](LICENSE) is retained. The upstream documentation is available in [README.upstream.md](README.upstream.md).
