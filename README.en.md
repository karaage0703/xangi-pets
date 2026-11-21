# xangi-pets

[![CI Build](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml/badge.svg)](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml)
[![GitHub Release](https://img.shields.io/github/v/release/karaage0703/xangi-pets)](https://github.com/karaage0703/xangi-pets/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[日本語](README.md)

A desktop companion for [xangi](https://github.com/karaage0703/xangi). It animates in a transparent always-on-top window, follows xangi's activity, and displays responses in speech bubbles. Transparent space around the pet passes clicks through to the applications below.

## Key features

- Animate with xangi's activity and show responses in speech bubbles
- Click the pet or press `t` to send messages, or open Web Chat
- Watch multiple xangi instances and automatically show each running session's agent
- Choose which session types to display: Chat, Web, and Automation
- Load `hatch-pet` compatible artwork from agent workspaces, with bundled `xangi` as fallback
- Adjust pet and bubble sizes, response display, and macOS notifications

## Quickstart

GitHub Releases currently provide a tested binary for macOS on Apple Silicon.

1. Download `xangi-pets_X.Y.Z_aarch64.dmg` from [Releases](https://github.com/karaage0703/xangi-pets/releases).
2. Copy the app to `/Applications`, then Control-click it in Finder and choose **Open**.
3. Add a connection when the app starts and enter the xangi URL. The default for xangi on the same Mac is `http://localhost:18888`.

For xangi on another machine, use a URL reachable over your LAN or Tailscale. xangi-pets can also connect to a headless xangi instance without its Web UI.

See [Installation](docs/INSTALL.md) for first-launch details and the [Usage Guide](docs/en/usage.md) for connections and controls.

## Start using xangi-pets

Click the pet or press `t` to send a message to xangi. Drag the pet to reposition it.

Use the menu bar or application menu to control visibility, connections, notifications, and sizes. If the pet moves off-screen, choose **Return Pet to Center**.

Choose types under **表示するセッション** (Session types) in the connection screen. Chat and Web are on by default; Automation is off. The setting applies to all connections, while permanent pets stay visible. See [Session pets](docs/en/usage.md#session-pets).

See the [Usage Guide](docs/en/usage.md) for keyboard controls, multiple instances, custom pets, and troubleshooting.

## Supported platforms

- macOS Apple Silicon: tested on real hardware and distributed as a `.dmg` on GitHub Releases
- Windows x86_64 / Linux x86_64: built in CI but not distributed because they have not been tested on real hardware

## Develop from source

Install Node.js 18 or later, stable Rust, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
npm ci
npm test
npm run tauri dev
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development and contribution workflow.

## Documentation

- [Usage Guide](docs/en/usage.md) — connections, menus, keyboard controls, multiple instances, custom pets, and troubleshooting
- [Installation](docs/INSTALL.md) — first launch on macOS, updates, and removal
- [Design Document](docs/en/design.md) — architecture, components, data flow, and design decisions
- [Event Protocol](docs/EVENTS.md) — xangi SSE events and the embedded API
- [CONTRIBUTING.md](CONTRIBUTING.md) — development setup and contribution workflow

## Related projects

- [xangi](https://github.com/karaage0703/xangi) — the main application for using AI agents from chat platforms and the Web UI
- [openai/skills hatch-pet](https://github.com/openai/skills/tree/main/skills/.curated/hatch-pet) — the source of the compatible sprite format

## License

Apache License 2.0. Distribution bundles include third-party license notices for bundled Rust crates and npm packages.
