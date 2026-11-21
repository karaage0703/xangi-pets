# Design Document

## Overview

xangi-pets is a Tauri 2 desktop app. Rust subscribes to xangi SSE streams, and the WebView renders animations and speech bubbles. Pets initiate connections, so xangi needs no per-pet connection configuration.

## Components

```text
xangi                            xangi-pets
/api/events/stream ── SSE ──▶ Rust: conversation aggregation ──▶ WebView: animation and bubbles
/api/sessions/stream ─ SSE ─▶ Rust: activity ──▶ session windows
/api/sessions ◀───────────── create a dedicated Web session
/api/pet/inbox ◀──────────── send messages
/api/workspace-file ───────▶ fetch agent sprites
```

- `src-tauri/src/`: startup, menus, notifications, settings, and window control
- `src-tauri/crates/events-server/`: SSE connections, aggregation, session monitoring, artwork, and sending
- `src/`: Canvas rendering, bubbles, connection settings, and input UI

## Connections and settings

Profiles store a name, URL, Web UI availability, optional `agentId`, and workspace-relative `spriteDirectory`. The profile list is shared between processes; each process chooses its input connection. Agent instructions, AI settings, and workspace assignments are managed by xangi.

The `enabled` flags are grouped by URL, with one monitor per enabled URL. The input connection has a permanent pet that accepts messages; other instances have display-only permanent pets. Disabling a URL stops its subscriptions and pets.

Settings storage:

- `connection-profiles.json` in the app configuration directory: connection profiles
- `pet-display.json` in the same directory: shared pet scale
- `session-filters.json` in the same directory: session types for all connections
- WebView localStorage: bubble settings, notifications, and permanent pet positions per profile

## Conversations and notifications

Rust aggregates `/api/events/stream` events by thread and exposes `/api/pet/state` and `/api/pet/bubbles` to the WebView.

- `turn.started`: `thinking`
- `message.delta`: `talking`
- `turn.complete` / `turn.aborted`: `idle`
- `agent.error`: `error`

Agent filtering happens before aggregation and notifications. It compares the event's `agent_id`, or resolves the thread's owning agent through the sessions API when absent. Results are pinned per turn; unresolved turns are not displayed.

Connection changes are serialized with sends. Once the outgoing subscription ends, conversation state, bubbles, and notification tracking are reset. Disconnects recover with exponential backoff. Notifications fire once for completion or error of turns whose start was observed on the current connection after notifications were enabled.

## Sending messages

Clicking the pet or pressing `t` opens the input UI. The first send creates a process-specific Web session through `POST /api/sessions`. The selected `agentId` and the outgoing `appSessionId` identify the conversation. Messages go to `POST /api/pet/inbox`, and responses arrive over SSE. The agent is checked before sending; missing agents produce an error.

The overall expression prioritizes `talking`, then `thinking`, then errors received within the last 30 seconds, then `idle`. Deadlines use a local monotonic clock; diagnostics and error bubbles remain intact. The state SSE stream reevaluates every second, emitting only expression changes.

## Session pets

`session_monitor.rs` subscribes to `/api/sessions/stream?mode=changes`. A `sessions` event replaces the full snapshot; `session_changes` and `activity_snapshot` update it. A pet is visible when `isActive=true`, `lifecycle!=closed`, and its type matches the selection in pets.

Sessions are classified in this order:

1. `scope=scheduler` or `executionOrigin=automation`: Automation (`scheduler`)
2. `platform=web`: Web (`web`)
3. Otherwise: Chat (`chat`)

Filters are saved by pets. Chat and Web are on by default; Automation is off. An empty array hides all session pets. Hidden rows are retained and reappear when enabled. After a successful save, a watch channel updates all monitors, re-evaluating the latest snapshot even while disconnected. New connections inherit the current selection. Source filters also apply before resident-pet aggregation and notifications. Agent ownership and source are resolved from the session API once per turn; visibility follows the current selection. Changing filters clears existing bubbles and expression state.

`session_windows.rs` keys windows by URL, permanent/session kind, and session ID. A disconnect alone does not close pets; reconnection reconciles the full snapshot. Display-only windows create neither input sessions nor notification subscriptions. The first rendered sprite and its label become visible together. Canvas dimensions follow the selected scale even during loading, so no label-only window is shown. Teardown releases timers and listeners.

## Artwork, sizes, and permissions

Artwork is fetched from the agent's workspace through `/api/workspace-file`, falling back to bundled `xangi` on failure. Relative paths, size limits, and WebP format are validated before bytes reach the WebView. See the [Usage Guide](usage.md#add-a-custom-pet) for placement and dimensions.

Scale changes serialize persistence, revision updates, window resizing, and event delivery; stale revisions are ignored. Permanent and session pets share drawing dimensions and hitboxes. Resizing preserves the bottom center and clamps at screen edges. A permanent pet with fewer than 48 visible pixels at startup returns to the monitor center.

Transparent areas pass clicks through. Web Chat opens in a separate window without the pet's Tauri permissions. Connection URLs accept only `http` / `https`, reject user information, and strip query strings and fragments.

See the [Event Protocol](../EVENTS.md) for fields and embedded APIs.
