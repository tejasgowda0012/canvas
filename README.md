<p align="center">
  <img src="branding/logo.png" alt="Inkboard" width="112">
</p>

<h1 align="center">Inkboard</h1>

<p align="center">
  A fast, native whiteboard for interactive displays, tablets, desktops and the web.<br>
  One Rust core, every platform, with live team boards.
</p>

<p align="center">
  <a href="https://whiteboard.kenpathlabs.com"><b>Download</b></a> ·
  <a href="https://whiteboard.kenpathlabs.com/app"><b>Open in browser</b></a> ·
  <a href="https://github.com/tejasgowda0012/inkboard/releases"><b>Releases</b></a>
</p>

---

Inkboard is built for writing by hand on large touch panels in classrooms and meeting rooms. Ink appears with very low latency, palms and resting hands are ignored, and several people can write at the same time. The same app runs on Android (including Android TV panels), macOS, Windows, Linux and in the browser, and team boards keep every device in sync live.

## Features

**Writing**
- Low-latency ink drawn through a front buffer, with optional stroke prediction and adaptive smoothing.
- Several fingers or pens can write at once.
- Palm and resting-hand rejection. On IR touch frames, the reported contact width tells the pen tip, the pen's back nib and a palm apart, with on-screen calibration.
- **Refine handwriting:** select ink to level lines, align words on a baseline and calm shaky strokes. It stays ink and is one undo step.
- Handwriting to typed text (English and Hindi, on-device with ML Kit) and automatic tidying of hand-drawn shapes.

**Board**
- Pen, highlighter, eraser, shapes, arrows, selection, move and resize, undo and redo.
- Tables, mind maps, formulas with auto-calculation, and graphs.
- Multiple pages per board, a movable toolbar and 36 bundled board fonts.
- Import images, SVG, PDF, PowerPoint, Word and Excel files. Export as PNG.

**Collaboration**
- **Team boards:** sign in with Google Workspace and edit the same board from many devices at once, with live cursors, selections, presence and per-shape authorship. Boards stay usable offline and sync on reconnect.
- Owner, editor and viewer roles, invitations by email, and organisation-wide sharing.
- **Phone as a writing pad:** write, erase and move the board on a large display from a phone.
- **Claude connector:** a built-in MCP server lets Claude read the board, add diagrams, text and shapes, and see a picture of the board.

## Platforms

| Platform | How to get it |
|---|---|
| Android and Android TV (API 24+) | APK from [Releases](https://github.com/tejasgowda0012/inkboard/releases) or [the website](https://whiteboard.kenpathlabs.com). Updates in-app. |
| macOS | Universal `.dmg` |
| Windows | `.exe` |
| Linux | Native binary |
| Web | [whiteboard.kenpathlabs.com/app](https://whiteboard.kenpathlabs.com/app), no install |

## Architecture

The whole app, including the editor, renderer, UI, menus and dialogs, is written once in Rust (`canvas_core`) and hosted by a thin layer on each platform.

```
rust/
├── core/          canvas_core: editor, ink, recognition, rendering, UI, sync client
├── android/       JNI host: surface, raw touch input, low-latency present, ML Kit
├── app-android/   Android project and build script
├── desktop/       winit + softbuffer host for macOS, Windows and Linux
└── web/           WebAssembly host drawing into a <canvas>
cloud/             Team-board backend on Cloudflare (Workers, Durable Objects, D1, R2)
```

Each team board is a Cloudflare Durable Object that orders changes, stores the board and relays presence. Every device keeps its own copy, so boards work offline.

More detail: [`rust/README.md`](rust/README.md) for the app and [`cloud/README.md`](cloud/README.md) for the backend.

## Building from source

Requires a stable Rust toolchain.

```sh
cd rust

# Run the core test suite
cargo test -p canvas_core

# Desktop app
cargo run --release -p inkboard_desktop

# Web app (served by the Worker at /app)
rustup target add wasm32-unknown-unknown
web/build.sh

# Android APK (needs the Android SDK and NDK)
bash app-android/build.sh
```

To run the backend locally:

```sh
cd cloud
npm ci
npx wrangler dev
```

CI builds the desktop apps, the Android APK and the web app, and publishes a GitHub release when the desktop version changes on `main`.

## Repository layout

| Path | Contents |
|---|---|
| `rust/` | The Inkboard app (current) |
| `cloud/` | Team-board backend and website |
| `branding/` | Logo sources and icon generator |
| `app/`, `server/` | Earlier Flutter app and Node sharing server, kept for reference |

## Branches

- `main` is the released app.
- `dev` is where new work is built and tested before it is merged into `main`.
