# Usage Guide

## Table of contents

- [Connect to xangi](#connect-to-xangi)
- [Basic controls](#basic-controls)
- [Menus](#menus)
- [Notifications and speech bubbles](#notifications-and-speech-bubbles)
- [Pets and display sizes](#pets-and-display-sizes)
- [Session pets](#session-pets)
- [Run multiple app instances](#run-multiple-app-instances)
- [Add a custom pet](#add-a-custom-pet)
- [Troubleshooting](#troubleshooting)

## Connect to xangi

The connection screen opens on first launch. Enter a name and the xangi event API URL, then choose whether that xangi instance also provides its Web UI.

- Default on the same Mac: `http://localhost:18888`
- Another machine: an `http://` or `https://` URL reachable over your LAN or Tailscale
- No Web UI: set `XANGI_EVENTS_SERVER_ENABLED=true` on xangi and turn off **Web UI available**

Connections are saved as profiles. Press `x` or use the **Connection** menu to add, select, edit, or disconnect a profile. Saved profiles are shared between multiple xangi-pets processes.

Use **同時に見守るインスタンス** (Watch instances together) to enable or disable each instance. Enabled instances show permanent and running session pets; disabling an instance closes its pets. Profiles sharing a URL are treated as one instance, and settings survive restart.

Select one input connection using its connection button. Saving and connecting enables that instance; the previous input instance continues to be watched if enabled. Other instances' permanent pets are display-only. After disabling the input instance, open Connections from the menu to choose another input destination. An enabled instance also shows its permanent pet while attempting to connect.

Use **Agent一覧を取得** (Load agents) to select an agent. The input connection's permanent pet displays that agent's conversations and sends messages through a dedicated session. You can save separate profiles for agents sharing a URL. **全Agentを見守る** (Watch all agents) displays all events and uses xangi's default settings for input. Agent selection requires xangi's agents and sessions APIs; use **Watch all agents** with an events-only server.

xangi-pets shows **Connected** after the SSE connection succeeds. It shows **Reconnecting** after a disconnect and reconnects automatically when xangi returns.

Only `http://` and `https://` URLs are accepted. URLs containing user information are rejected, and query strings and fragments are removed before storage.

## Basic controls

- Click the pet or press `t`: send a message to xangi
- Drag: move the pet
- Click a speech bubble: dismiss it
- `x`: select or configure a connection
- `p`: cycle the shared size of all pets
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

Artwork is loaded from the agent's workspace. A permanent pet with no selected agent uses the default xangi agent's artwork. If loading fails, it uses bundled `xangi`. See [Add a custom pet](#add-a-custom-pet) for file placement.

Press `p` on any pet to change the shared size of all pets. Newly opened pets inherit it, and the setting survives restart.

- Shared pet size: `1.0 / 1.5 / 2.0 / 2.5 / 3.0 / 4.0`
- Bubble size: `1.0 / 1.5 / 2.0 / 2.5 / 3.0 / 4.0`

Bubble size, notifications and permanent pet positions are saved per connection profile.

## Session pets

Running sessions on the connected xangi automatically open temporary pets alongside the permanent pet. Two sessions for the same agent show two pets. Each window displays its agent's sprite, agent name, session title and status, and can be dragged. This covers the entire connected instance regardless of the permanent pet's selected agent.

Press `x` or choose **Connection** from the menu, then toggle Chat, Web, and Automation under **表示するセッション** (Session types). Chat and Web are on by default; Automation is off. Changes apply immediately to session pets across all connections and survive restart. Turning all types off hides session pets. Permanent pets stay visible. Their active status is labeled 「見守り中」 (watching), separately from running sessions. Responses, expressions and notifications use the same source filters. Changing the selection clears current bubbles.

Completion, cancellation or error closes the affected pet once its session stops running. Starting another turn opens it again. Sessions already running at connection time are included.

Temporary pets are display-only: use the input connection's permanent pet for messages, and no duplicate notifications are generated. Show/hide menus affect all pets. Disconnecting closes that instance's pets while other enabled instances continue. A network interruption alone does not imply completion; reconnecting reconciles against the latest monitor snapshot. This feature requires `/api/sessions/stream?mode=changes` on xangi.

## Run multiple app instances

One app process normally suffices to watch multiple xangi instances. You can run multiple copies of the built app on one Mac; each displays its enabled instances.

```bash
open -n -a /Applications/xangi-pets.app
open -n -a /Applications/xangi-pets.app
```

The embedded server starts at port `7895` and automatically moves to the next available port. The active port appears in the menus.

Multiple concurrent runs of `npm run tauri dev` are not supported because Vite uses fixed port `1420`.

## Add a custom pet

An agent connection first loads sprites from that agent's workspace. The default layout is:

```text
<Agent workspace>/assets/xangi-pets/
├── pet.json
└── spritesheet.webp
```

Example `pet.json`:

```json
{"id":"my-pet","displayName":"My Pet","spritesheetPath":"spritesheet.webp"}
```

For a remote xangi, put the files in the workspace on the xangi machine. The sprite directory in the connection form is relative to the agent's workspace. Agents sharing a workspace can use separate relative directories. If loading fails, the pet explains the failure and uses bundled `xangi`. Change artwork by selecting an agent or updating the workspace files. A deleted agent causes a connection/send error. Restart or reselect the connection to reload changed images.

Files are fetched through xangi's workspace file API: at most 64 KiB for `pet.json` and 16 MiB for WebP. The required image dimensions are listed below. Absolute paths and parent directory references are rejected.

The sprite sheet is a transparent 1536×1872 WebP atlas with 8 columns, 9 rows, and 192×208 cells.

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

## Expression after an error

The sad expression returns to normal about 30 seconds after an error is received. Thinking or talking in another conversation takes priority. Error bubbles remain available after the expression recovers.
