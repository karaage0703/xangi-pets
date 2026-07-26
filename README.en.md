# xangi-pets

[![CI Build](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml/badge.svg)](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml)
[![GitHub Release](https://img.shields.io/github/v/release/karaage0703/xangi-pets)](https://github.com/karaage0703/xangi-pets/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[日本語](README.md)

A small always-on-top desktop pet for [xangi](https://github.com/karaage0703/xangi). It animates in a transparent window, follows xangi's activity state, and displays responses in speech bubbles without blocking clicks on the rest of your desktop.

## Features

- Animations for xangi's `idle`, `thinking`, `talking`, and `error` states
- Speech bubbles for concurrent conversations
- Long-message paging every four seconds, aligned to complete text lines
- Send a message by clicking the pet or pressing `t`
- Five pet sizes and five bubble sizes
- Codex `hatch-pet` compatible sprites and a bundled original `xangi` pet
- A release bundle verified on macOS with Apple Silicon

## Quick start

1. On a Mac with Apple Silicon, download the `.dmg` from [Releases](https://github.com/karaage0703/xangi-pets/releases).
2. Install and launch it. See [Installation](docs/INSTALL.md) for unsigned-app warnings.
3. Enter the xangi Web Chat URL when prompted. The local default is `http://localhost:18888`.

For xangi on another machine, use a Web Chat URL reachable over your LAN or Tailscale. Press `x` to change it later.

## Supported release

GitHub Releases currently provide only the macOS Apple Silicon `.dmg`, which has been tested on real hardware. Windows x86_64 and Linux x86_64 packages are built in CI, but they have not been tested on real hardware and are not included in releases.

## Controls

| Key or action | Behavior |
|---|---|
| Click pet / `t` | Send a message to xangi |
| Drag pet | Move the window |
| `x` | Change the xangi URL |
| `c` | Choose a pet |
| `b` | Cycle bubble size |
| `p` | Cycle pet size |
| `h` / `?` | Toggle help |
| Click bubble | Dismiss the bubble |

Long responses show four complete lines per page. Paging starts after the response completes, advances every four seconds, visits the final page, and then loops to the beginning.

## Custom pets

The app includes the original `xangi` sample pet, so no asset setup is required. Personal character assets are not included in the public distribution. Custom Codex `hatch-pet` compatible assets can be placed in either location:

```text
~/.xangi/pets/<pet-name>/
~/.codex/pets/<pet-name>/
├── pet.json
└── spritesheet.webp
```

The sprite sheet is a transparent 1536×1872 WebP atlas with 8 columns, 9 rows, and 192×208 cells.

## Development

Install Node.js 18 or later, stable Rust, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
npm ci
npm test
npm run tauri dev
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution workflow and [docs/EVENTS.md](docs/EVENTS.md) for the event protocol.

## Architecture

```text
xangi Web Chat
  └─ GET /api/events/stream (SSE)
       └─ xangi-pets embedded Rust server
            ├─ state aggregation
            └─ speech-bubble events → Tauri webview
```

The pet initiates the SSE connection. Multiple pets can connect without adding callback URLs to xangi.

## License

Apache License 2.0. Distribution bundles include generated third-party license notices.
