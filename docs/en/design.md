# Design Document

## Overview

xangi-pets is a Tauri 2 desktop application. It pulls events from xangi over SSE and converts them into pet animations and speech bubbles. Multiple pets can connect without registering a callback URL for each pet in xangi.

## Architecture

```text
xangi
  ├─ GET /api/events/stream ── SSE ──▶ xangi-pets Rust backend
  ├─ POST /api/sessions ◀───────────── create a dedicated Web session
  └─ POST /api/pet/inbox ◀──────────── send a message from the pet
                                          │
                                          ▼
                                 embedded event server
                                   ├─ thread state aggregation
                                   ├─ /api/pet/state
                                   └─ /api/pet/bubbles
                                          │
                                          ▼
                                    Tauri webview
                                   Canvas + speech bubbles
```

Web Chat is displayed in a separate normal window. Remote Web Chat content does not receive the Tauri permissions assigned to the pet webview.

## Components

### Tauri application (`src-tauri/src/`)

Manages application startup, the transparent always-on-top window, menu-bar and application menus, connection profiles, notifications, and window position and size. This layer also handles click-through areas and returning an off-screen pet to the center of the display.

### Event server (`src-tauri/crates/events-server/`)

Subscribes to xangi SSE events and aggregates turn events by thread. It exposes the aggregated state and speech-bubble streams to the pet webview. It also sends pet input to xangi through a dedicated Web session.

### Frontend (`src/`)

Uses Vite and vanilla JavaScript. It renders sprite animations on Canvas, switches animations based on state, pages speech bubbles, presents the input modal, and selects pets.

### Connection profiles (`src/lib/connection-profiles.js`)

Stores the xangi name, event API URL, and whether its Web UI is available. The profile list is shared between processes, while each process keeps its selected connection and display settings separate.

### Window layout (`src/lib/window-layout.js`)

Calculates the required window dimensions from pet and bubble scales. Position is stored per profile. On startup, a pet with fewer than 48 visible pixels is returned to the center of the current monitor. The user can also invoke the same recovery explicitly from a menu.

## Data flow

### Display a xangi response

1. The Rust backend connects to `GET /api/events/stream`.
2. The event server aggregates `turn.started`, `message.delta`, `turn.complete`, and related events by thread.
3. `/api/pet/state` publishes the overall state and `/api/pet/bubbles` publishes speech-bubble events.
4. The webview renders the corresponding sprite row and speech bubbles.

State is derived by the consumer from wire events.

- After `turn.started`, before a delta: `thinking`
- After `message.delta`: `talking`
- After `turn.complete` or `turn.aborted`: `idle`
- After `agent.error`: `error`

### Send a message from the pet

1. A click or the `t` key opens the input modal.
2. On the first send, `POST /api/sessions` creates a Web session dedicated to that process.
3. The message is sent to `POST /api/pet/inbox` with its `appSessionId`.
4. The response returns through the normal SSE path and appears in a speech bubble.

The dedicated session prevents pet messages from joining browser or other device sessions.

## Design decisions

### Pull-based connections

The pet connects to xangi, so adding pets requires no callback configuration in xangi. Connections recover with exponential backoff after a disconnect.

### Separate transport from presentation

The Rust backend owns event transport and aggregation. The webview renders already aggregated data and does not need to understand the wire protocol.

### Isolate settings by connection

Character, size, notifications, and position are stored per connection profile. Multiple xangi instances and pet processes can run without mixing their settings.

### Avoid obstructing desktop interaction

The window is transparent and always on top, but areas outside the pet pass clicks through. The pet can move freely and recover from an off-screen position either automatically or through a menu action.

### Isolate permissions for remote content

In-app Web Chat contains remote content from the configured connection. It does not receive the pet frontend's Tauri capability. URLs are limited to `http` and `https`; user information is rejected, and query strings and fragments are removed.

## Key file structure

```text
xangi-pets/
├── src/                         # Canvas UI, bubbles, and connection profiles
├── src-tauri/
│   ├── src/                     # Tauri lifecycle, menus, and window control
│   ├── crates/events-server/    # xangi SSE client, aggregation, and pet inbox
│   ├── capabilities/            # Tauri capability
│   └── resources/default-pets/  # bundled pet
├── docs/                        # usage, design, event, and installation docs
├── scripts/                     # tests and license generation
└── release-scripts/             # public-release preparation and verification
```

See the [Event Protocol](../EVENTS.md) for event and endpoint details.

## Extension points

### Add a pet

Place `pet.json` and `spritesheet.webp` in the Codex `hatch-pet` compatible format. Rebuilding the application is not required.

### Add a xangi event

Update the wire schema, Rust event model, thread aggregation, and webview-facing aggregate events in that order. Preserve backward compatibility and guard against duplicate notifications after reconnecting.

### Distribute on another operating system

Do not treat a successful CI build as release readiness. Verify transparent windows, click-through behavior, menus, notifications, and position recovery across multiple monitors on real hardware.
