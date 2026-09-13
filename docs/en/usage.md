# Usage Guide

## Table of contents

- [Connect to xangi](#connect-to-xangi)
- [Basic controls](#basic-controls)
- [Menus](#menus)
- [Notifications and speech bubbles](#notifications-and-speech-bubbles)
- [Pets and display sizes](#pets-and-display-sizes)
- [Run multiple pets](#run-multiple-pets)
- [Add a custom pet](#add-a-custom-pet)
- [Troubleshooting](#troubleshooting)

## Connect to xangi

The connection screen opens on first launch. Enter a name and the xangi event API URL, then choose whether that xangi instance also provides its Web UI.

- Default on the same Mac: `http://localhost:18888`
- Another machine: an `http://` or `https://` URL reachable over your LAN or Tailscale
- No Web UI: set `XANGI_EVENTS_SERVER_ENABLED=true` on xangi and turn off **Web UI available**

Connections are saved as profiles. Press `x` or use the **Connection** menu to add, select, edit, or disconnect a profile. Saved profiles are shared between multiple xangi-pets processes.

xangi-pets shows **Connected** after the SSE connection succeeds. It shows **Reconnecting** after a disconnect and reconnects automatically when xangi returns.

Only `http://` and `https://` URLs are accepted. URLs containing user information are rejected, and query strings and fragments are removed before storage.

## Basic controls

- Click the pet or press `t`: send a message to xangi
- Drag: move the pet
- Click a speech bubble: dismiss it
- `x`: select or configure a connection
- `c`: select a pet
- `p`: cycle the pet size
- `b`: cycle the speech-bubble size
- `h` or `?`: toggle help

Transparent areas pass clicks through to applications below. Keys `1` through `9` are reserved for animation previews during development.

## Menus

xangi-pets remains available in the macOS menu bar while running. The normal application menu, available after selecting xangi-pets in the Dock, provides the same primary actions.

- Show or hide the pet
- Return the pet to the center of the display
- Talk to xangi
- Open Web Chat in the app or default browser
- Configure a connection
- Toggle live responses, completion messages, and system notifications
- Change the pet and bubble sizes
- Show help or quit the app

If automatic wandering, manual placement, or a monitor-layout change carries the pet off-screen, choose **Return Pet to Center**. On startup, xangi-pets also returns the pet to the current monitor's center when almost none of it is visible.

## Notifications and speech bubbles

Live-response and completion-message bubbles can be toggled independently. This allows a completion-only setup that stays quiet while xangi is working.

macOS system notifications are off by default. Once enabled, xangi-pets sends one notification for the completion or error of a turn that started after notifications were enabled. Historical events received after reconnecting do not trigger notifications. macOS may request permission when the first notification is shown.

Long responses show four lines per page. Paging begins after the response completes, advances every four seconds, and returns to the first page after the last one.

## Pets and display sizes

The bundled `xangi` pet works without additional setup. Press `c` to choose from available pets.

- Pet sizes: `0.5 / 0.8 / 1.1 / 1.5 / 2.0`
- Speech-bubble sizes: `1.0 / 1.3 / 1.6 / 2.0 / 2.5`

Size, notification settings, and screen position are stored per connection profile.

## Run multiple pets

You can run multiple copies of the built app on one Mac and assign them to different xangi instances or characters.

```bash
open -n -a /Applications/xangi-pets.app
open -n -a /Applications/xangi-pets.app
```

The embedded server starts at port `7895` and automatically moves to the next available port. The active port appears in the menus.

Multiple concurrent runs of `npm run tauri dev` are not supported because Vite uses fixed port `1420`.

## Add a custom pet

Place Codex `hatch-pet` compatible assets in either location. The first path has priority.

```text
~/.xangi/pets/<pet-name>/
~/.codex/pets/<pet-name>/
├── pet.json
└── spritesheet.webp
```

The sprite sheet is a transparent 1536×1872 WebP atlas with 8 columns, 9 rows, and 192×208 cells. Press `c` after launch to select the new pet.

Set `XANGI_PET_DIR` to restrict discovery to a single directory. When set, other search paths and the bundled pet are ignored.

You can create compatible assets with [hatch-pet from openai/skills](https://github.com/openai/skills/tree/main/skills/.curated/hatch-pet).

## Troubleshooting

### The pet moved off-screen

Choose **Return Pet to Center** from the menu bar or normal application menu.

### The menu-bar icon is missing

Select xangi-pets in the Dock and use the normal application menu at the top-left of the screen. The primary actions are available in both menus.

### The status never becomes Connected

Confirm that xangi is running and that its configured URL is reachable from this Mac. For another machine, also check LAN or Tailscale connectivity. A headless xangi instance requires `XANGI_EVENTS_SERVER_ENABLED=true`.

### Messages cannot be sent from the pet

Use a xangi version that supports `POST /api/pet/inbox`. For a xangi instance exposed through a global IP, set the same `XANGI_PET_INBOX_TOKEN` in xangi and xangi-pets. Loopback, LAN, and Tailscale connections normally do not require this token.

### macOS refuses to open the app

See [Installation](../INSTALL.md) for Gatekeeper instructions.

### I need event-protocol details

See the [Event Protocol](../EVENTS.md).
