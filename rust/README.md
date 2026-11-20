# Inkboard app (Rust)

The library screen, floating editor toolbars, style panel, menus, dialogs, text/icon drawing and TV remote focus are rendered by Rust. Their layout follows the Flutter app: menu/undo at top left, styles at top right, zoom at bottom left, tools at bottom centre. There are no Kotlin screen or toolbar implementations. A small Android host supplies the Surface, raw input, IME, complex-script text rasterisation, ML Kit and system installation APIs.

## Included

- Pen, handwriting-to-text pen, smart-shape pen, selection/move/resize, marquee selection, eraser, arrows, rectangles and ellipses.
- Automatic shape tidying and handwriting-to-typed-text conversion, with separate persisted settings. English and Hindi. There is no keyboard: board text comes from handwriting or from Claude.
- 36 board fonts (31 bundled open-licence families, see `assets/licenses`) with a font picker that previews each one; colour, width and fill controls.
- Export as PNG to Pictures/Inkboard.
- Team boards: sign in with a Google Workspace account and edit the same board from many devices at once, like Figma. Everyone keeps their own view; changes, cursors and selections appear live, and each shape records who added it and who changed it last. See [Team boards](#team-boards).
- Claude connector: the panel is a Streamable HTTP MCP server (`/mcp/<key>` on port 18763, on the local network). Claude can read the open board, add and update shapes, text and diagrams, delete with confirmation and get a PNG picture of the board. Suggested models: Claude Opus 5.5 for diagrams, Claude Sonnet 5.5 for quick edits.
- Palm rejection, bounded multi-touch forwarding (five contacts), historical/unbuffered input.
- 1920-pixel maximum drawing-buffer side by default for 4K panels. Full-resolution mode is optional. Only dirty screen rectangles are copied while writing.
- Atomic saved board files, rename/delete, thumbnails, undo/redo, zoom/fit. Leaving a dirty board waits for the write; a failed write keeps the board open. Backgrounding preserves a stroke already in progress.
- GitHub OTA checks for `rust-v*` releases, with package/version/signing-certificate validation and Android confirmation before installation.
- Android API 24+, ARM 32-bit, ARM64, x86 and x86_64 in the same APK.

The package is `com.canvasstudio.rust`, separate from the Flutter app. Earlier Rust boards remain in the same app-private `files/boards` directory. Flutter boards are not automatically imported. The app is shown as Inkboard; the package name is unchanged so over-the-air updates keep working.

## Insert files

Open a board and select **+ (Insert)** in the dock.

- **Image** inserts a movable, resizable image from PNG, JPEG, WebP, GIF (first frame), or BMP.
- **PDF → New pages** appends one board page per PDF page and locks each background against touch selection and erasing. **PDF → Current page** places the pages below one another as movable images.
- **Document** imports SVG as an image, PPTX as editable slide pages, DOCX as flowing text, images, and tables, and the first XLSX worksheet as editable tables. XLSX formulas use their saved results. Complex Office layouts can lose formatting, and unsupported slide objects can fall back to text.

The Android picker accepts files up to 32 MiB. PDF import accepts up to 80 pages and uses Android's rasterizer, with a 1,920-pixel longest side and a 32 MiB total raster payload. PPTX import accepts up to 80 slides. Large worksheet and document tables are split to fit Inkboard's table editor.

Local board JSON stores an `imageRef` for each image. The matching SHA-256-named PNG or JPEG bytes live in the board's sibling `<id>.assets` directory, which belongs with the JSON when copying the app-private files. Board sharing sends the image bytes as base64 `imageData` inline, so a joined device needs only the normal sharing connection. Sharing and PNG export use the open page.

## Team boards

Team boards live on the Inkboard server in [`cloud/`](../cloud/README.md): a Cloudflare Worker with one Durable Object per board, D1 for accounts, boards and permissions, and R2 for images. There is no Tailscale relay or peer hosting any more.

- **Sign in**: Library → *Sign in* (or the menu → *Sign in…*). The app shows a code and a QR link; approve it in any browser with your Workspace Google account. Personal Gmail and other organisations are refused.
- **Make or move a board**: on the *Team boards* tab, *New board* makes a team board. A board on this device moves with its menu → *Copy to Team boards*, or the board's Share button.
- **Share**: the board's Share button lists who is on the board now, who has access (owner, can edit, can view) and lets the owner invite people by email or open the board to everyone in the organisation.
- **Live editing**: every device keeps its own copy, camera and tools. Changes go to the board's room, which orders them; the last change to a shape wins. Others' cursors (with names), selections and fresh edits are drawn on the board; their avatars sit in the top-right bar. Select a shape to see *Added by … · edited by …*.
- **Offline**: a team board is cached as `cloud-<id>` in the boards directory and keeps working offline; changes made while away are sent on reconnect.
- **Undo** only undoes your own changes.
- **View-only** people can watch, see cursors and export, but changes are put back.

The client code is `core/src/ui/cloud/` (HTTP, WebSocket room connection, sync engine) and `core/src/ui/app/team.rs` (screens, presence, sharing). End-to-end tests run two apps against `wrangler dev`; see `.github/workflows/cloud.yml`.

## Desktop (macOS, Windows, Linux)

`desktop/` is a native window host for the same `canvas_core` app: winit for the window and input (mouse, pen, touch, trackpad pinch, keyboard), softbuffer to present the frames, cosmic-text for board text with the same bundled typefaces (with Devanagari shaping), and system file dialogs.

```sh
cargo run --release -p inkboard_desktop
```

Mouse wheel pans, Ctrl/⌘ + wheel or pinch zooms, Ctrl/⌘+Z / Shift+Ctrl/⌘+Z undo and redo, Delete removes the selection, Esc goes back. `INKBOARD_UI_SCALE` (default 1.2) sizes the interface; `INKBOARD_SERVER` points at another team-board server. Boards and settings live in the platform data folder (`~/Library/Application Support/Inkboard`, `%APPDATA%\Inkboard`, `~/.local/share/Inkboard`).

Not on desktop yet: handwriting-to-text (Android uses ML Kit; Windows Ink and macOS Vision are the planned replacements), PDF page import, and in-app updates. `.github/workflows/desktop.yml` builds a universal macOS `.dmg` (ad-hoc signed), a Windows `.exe`, a Linux binary and the Android `.apk`. When `version` in `rust/desktop/Cargo.toml` changes on `main`, it publishes GitHub release `v<version>`. https://whiteboard.kenpathlabs.com always links to the latest release. Silent installs need an Apple Developer ID for notarisation and a Windows code-signing certificate. Android release signing comes from the `ANDROID_KEYSTORE_B64`, `ANDROID_STORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD` repository secrets; without them the APK uses a debug key.

## Web

`web/` runs the same app in a browser at **https://whiteboard.kenpathlabs.com/app**: `canvas_core` compiled to WebAssembly, drawing into a `<canvas>`. Team boards, live collaboration, sign-in, local boards, images, PDF import (pages rendered with pdf.js) and PNG export all work. Local boards are saved in the browser's IndexedDB.

What differs in a browser (`core/src/sys.rs`, `core/src/ui/cloud/`):
- The page's clock replaces the system clock.
- Disk work runs at the start of a frame instead of on a thread.
- Files live in memory and the page copies them to IndexedDB.
- Server requests use `fetch`. The board room uses the page's WebSocket, with the session token sent as the `bearer.<token>` subprotocol.
- Board fonts are fetched as separate files.
- The phone pad and the local-network Claude connector are left out.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <the version in Cargo.lock>
web/build.sh            # writes cloud/public/app, which the Worker serves at /app
cd ../cloud && npx wrangler dev   # then open http://localhost:8787/app/
```

## Build

Build host: `ssh addypy` (LAN) or `ssh addypy-ts` (Tailscale).
Source checkout: `/home/ubuntu/canvas-rust`.

```sh
cargo test -p canvas_core
bash app-android/build.sh
```

The script cross-compiles four Android ABIs and signs/packages `out/canvas-studio-rust.apk`. Keep the existing release keystore and `app-android/key.properties` private; neither belongs in git. Future Rust updates must use that key and an increased Android version code.

## Validation

`cargo test -p canvas_core` runs the core suite (about 160 tests): ink input, palm rejection and multi-finger writing, shape and handwriting refinement, rendering against full repaints, saving and reopening boards, and the UI. Team-board sync is tested end to end against `wrangler dev` in CI.

## Download

Releases are published on [GitHub](https://github.com/tejasgowda0012/inkboard/releases) and linked from https://whiteboard.kenpathlabs.com. Installed apps check for updates themselves.
