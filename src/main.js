// xangi-pets renderer
//
// Asset and state come from xangi-pets-server (HTTP + SSE) when available.
// Falls back to bundled `/pets/xangi/` when the server is unreachable, so the
// app still works for local dev or first launch before the server is set up.

import { bubblePageLayout, makeBubbleUI, subscribeBubbles } from './lib/bubble.js';
import { makeClickGateController } from './lib/click-gate.js';
import { fitWindowSize } from './lib/window-layout.js';

const CELL_W = 192;
const CELL_H = 208;
const COLS = 8;
const ROWS = 9;

// Lower bound on the window width — even when a tiny pet + tiny bubble
// would technically fit in less, keep at least this so picker overlays and
// drag handles don't feel cramped.
const BASE_WINDOW_W = 280;

// Allowed bubble-scale values cycled by the `b` key. 1.0 = baseline (matches
// the original fixed sizing). Steps biased toward "bigger" because the use
// case for shrinking bubbles is rare — most cycles are about making text
// readable on a projected demo screen.
const BUBBLE_SCALE_STEPS = [1.0, 1.5, 2.0, 2.5, 3.0, 4.0];

// Allowed pet-scale values cycled by the `p` key. 1.0 = baseline (= the size
// you see at first launch). Numbers are user-visible multipliers; the actual
// rendered pixel size is multiplied by PET_RENDER_FACTOR below to map the
// hatch-pet 192×208 native sprite to the historic baked-in render size.
const PET_SCALE_STEPS = [1.0, 1.5, 2.0, 2.5, 3.0, 4.0];

// User-visible 1.0x renders at half the native sprite cell. Keeps the first-
// launch appearance identical to the historic 0.5 default while letting the
// `p` key cycle in clean human-readable numbers (1x / 1.5x / 2x / …).
const PET_RENDER_FACTOR = 0.5;

// Vertical room for the bubble stack at bubble-scale = 1. The original window
// (200h) - pet (104h at scale=0.5) leaves ~96px of bubble space; we use that
// as the baseline and scale it linearly with bubble-scale.
const BUBBLE_AREA_H_BASE = 96;
const BUBBLE_MAX_W_BASE = 240; // matches .bubble's max-width in styles.css
const WINDOW_HORIZONTAL_PADDING = 40;

// Animation row layout: hatch-pet `references/animation-rows.md` convention.
const ROW_NAMES = [
  'idle',          // 0: neutral breathing/blink
  'running-right', // 1
  'running-left',  // 2
  'waving',        // 3
  'jumping',       // 4
  'failed',        // 5
  'waiting',       // 6
  'running',       // 7
  'review',        // 8: focused/thinking loop
];

// Map xangi state -> row name.
const STATE_TO_ROW = {
  idle: 'idle',
  thinking: 'review',
  talking: 'waving',
  error: 'failed',
};

const FPS = 6;

// Namespaced localStorage. Multiple xangi-pets processes (e.g. two `open -n`
// instances) share the same WKWebView storage on macOS, so unprefixed keys
// would clobber each other and force both windows to show the same pet /
// bubble scale. The namespace is derived from the embedded server's bound
// port (which is auto-shifted per process), so each instance gets its own
// view — but reads fall back to the legacy unprefixed key on first launch
// so existing users don't lose their saved pet selection.
const STORAGE_PREFIX = 'xangi-pets';
let storageNamespace = ''; // assigned once we know the bound port

function readStorage(key) {
  const ls = globalThis.localStorage;
  if (!ls) return null;
  if (storageNamespace) {
    const scoped = ls.getItem(`${STORAGE_PREFIX}:${storageNamespace}:${key}`);
    if (scoped !== null) return scoped;
  }
  return ls.getItem(`${STORAGE_PREFIX}:${key}`);
}

function writeStorage(key, value) {
  const ls = globalThis.localStorage;
  if (!ls) return;
  const ns = storageNamespace || '';
  const fullKey = ns
    ? `${STORAGE_PREFIX}:${ns}:${key}`
    : `${STORAGE_PREFIX}:${key}`;
  ls.setItem(fullKey, value);
}

function removeStorage(key) {
  const ls = globalThis.localStorage;
  if (!ls) return;
  if (storageNamespace) {
    ls.removeItem(`${STORAGE_PREFIX}:${storageNamespace}:${key}`);
  }
  ls.removeItem(`${STORAGE_PREFIX}:${key}`);
}

function deriveNamespace(serverUrl) {
  try {
    const u = new URL(serverUrl);
    // Port is the cleanest discriminator across instances on the same host.
    // Falls back to host when port is missing (e.g. a remote server URL).
    return u.port || u.hostname || '';
  } catch {
    return '';
  }
}

// xangi URL — the upstream xangi instance the pet pulls events from
// (`GET /api/events/stream`). Distinct from `serverUrl`, which is the
// pet's own embedded localhost server (assets / bubble SSE / etc).
//
// Storage is global (unprefixed) and used as a UI default. Each pet
// process holds its own xangi URL in Rust memory, so two pet windows
// pointed at different xangi instances stay separate at runtime even
// though they share this localStorage entry. To start two windows with
// different xangi URLs, launch them with the `XANGI_URL` env var set
// per-process, or set them sequentially via the `x` key after launch.
const XANGI_URL_KEY = `${STORAGE_PREFIX}:xangi-url`;

function readXangiUrl() {
  const fromStorage = globalThis.localStorage?.getItem(XANGI_URL_KEY) ?? '';
  if (fromStorage) return fromStorage;
  const fromEnv = import.meta.env?.VITE_XANGI_URL;
  return fromEnv || '';
}

function writeXangiUrl(value) {
  if (!globalThis.localStorage) return;
  if (value) {
    globalThis.localStorage.setItem(XANGI_URL_KEY, value);
  } else {
    globalThis.localStorage.removeItem(XANGI_URL_KEY);
  }
}

async function tauriInvoke(name, args) {
  const tauri = await import('@tauri-apps/api/core');
  return tauri.invoke(name, args);
}

/**
 * Make sure the Rust pull client is subscribed to a xangi instance.
 *
 * 1. If the Rust side already has a URL (e.g. XANGI_URL env was set), reuse it.
 * 2. Otherwise read the saved value from localStorage and push it down.
 * 3. Otherwise prompt the user once. Cancelling leaves bubbles offline but
 *    lets the rest of the pet (sprite, key bindings) start so the user can
 *    re-enter the URL with the `x` key later.
 *
 * Returns the URL we ended up applying, or `null` when no URL is configured.
 */
async function ensureXangiUrl() {
  // 1. Rust side already has a URL?
  try {
    const cur = await tauriInvoke('get_xangi_url');
    if (typeof cur === 'string' && cur) {
      // Mirror to localStorage so the prompt next time has the right default.
      writeXangiUrl(cur);
      return cur;
    }
  } catch {
    // Browser dev mode — no Tauri commands available.
    return readXangiUrl() || null;
  }

  // 2. localStorage / env fallback.
  const saved = readXangiUrl();
  if (saved) {
    try {
      const applied = await tauriInvoke('set_xangi_url', { url: saved });
      if (typeof applied === 'string' && applied) {
        return applied;
      }
    } catch (err) {
      console.warn(`xangi-pets: saved xangi URL ${saved} rejected by Rust: ${err}`);
    }
  }

  // 3. Show the in-app modal. We can't use window.prompt() — Tauri 2's
  //    WKWebView blocks it (no-ops silently), which used to make the `x` key
  //    feel broken. The custom modal also lets us offer a "接続解除" button.
  const result = await pickXangiUrl(saved);
  if (!result || result.action === 'cancel') {
    console.warn('xangi-pets: xangi URL not configured. Press `x` to set it later.');
    return null;
  }
  if (result.action === 'disconnect') {
    writeXangiUrl('');
    return null;
  }
  const entered = result.url;
  if (!entered) return null;
  try {
    const applied = await tauriInvoke('set_xangi_url', { url: entered });
    writeXangiUrl(typeof applied === 'string' ? applied : entered);
    return typeof applied === 'string' ? applied : entered;
  } catch (err) {
    console.error(`xangi-pets: set_xangi_url failed: ${err}`);
    return null;
  }
}

// localStorage / env override take precedence (so the user can point the
// frontend at a remote server if they want). Otherwise we ask the embedded
// Tauri server for the port it actually bound to — the auto-shift logic
// means we can't hard-code 7895.
async function resolveServerUrl() {
  // Server URL is intentionally NOT namespaced — pointing different instances
  // at different servers would defeat the multi-pet setup. Read the legacy
  // unprefixed key directly.
  const fromStorage = globalThis.localStorage?.getItem(`${STORAGE_PREFIX}:server`);
  if (fromStorage !== null && fromStorage !== undefined && fromStorage !== '') {
    return fromStorage;
  }
  const fromEnv = import.meta.env?.VITE_XANGI_PET_SERVER;
  if (fromEnv) return fromEnv;

  // Tauri shell: ask the Rust side for the actual bound URL.
  for (let attempt = 0; attempt < 20; attempt++) {
    try {
      const tauri = await import('@tauri-apps/api/core');
      const url = await tauri.invoke('get_server_url');
      if (typeof url === 'string' && url) return url;
    } catch {
      // not running inside Tauri — fall through to default
      break;
    }
    // Server still binding. Wait briefly and retry.
    await new Promise((r) => setTimeout(r, 100));
  }
  // Default fallback (browser dev mode, or Tauri command unavailable).
  return 'http://127.0.0.1:7895';
}

// No default name. The first time the user runs xangi-pets they pick from
// whatever sprites are present in ~/.xangi/pets / ~/.codex/pets via the
// in-window picker. The selection is persisted to localStorage afterwards.
function readPetName() {
  const fromStorage = readStorage('name');
  return fromStorage && fromStorage.length > 0 ? fromStorage : null;
}

function readScale() {
  // Pet scale is now a user-visible multiplier (1.0 = first-launch
  // appearance). Anything below 1.0 is treated as a stale legacy value
  // (the old default was 0.5) and snapped back to the new baseline so
  // returning users see the expected size on next launch.
  const fromStorage = readStorage('scale');
  const fromEnv = import.meta.env?.VITE_XANGI_PET_SCALE;
  const raw = fromStorage ?? fromEnv ?? '1';
  const n = Number(raw);
  return Number.isFinite(n) && n >= 1.0 && n <= 6 ? n : 1.0;
}

// Convert a user-visible pet scale (1.0 = first-launch appearance) into the
// pixel dimensions we actually render the sprite at. Centralised so the
// canvas, the Tauri window-size calculation, and the Rust hit-test rectangle
// all agree.
function petPixelW(petScale) {
  return Math.round(CELL_W * PET_RENDER_FACTOR * petScale);
}
function petPixelH(petScale) {
  return Math.round(CELL_H * PET_RENDER_FACTOR * petScale);
}

function readBubbleScale() {
  const fromStorage = readStorage('bubble-scale');
  const fromEnv = import.meta.env?.VITE_XANGI_PET_BUBBLE_SCALE;
  const raw = fromStorage ?? fromEnv ?? '1';
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0.5 && n <= 3 ? n : 1.0;
}

// Compute window dimensions (logical px) that fit both the pet sprite and
// the bubble stack at the given scales. Width takes the wider of pet sprite
// or scaled bubble; height stacks the pet (anchored at bottom) plus scaled
// bubble area on top.
function computeWindowSize(petScale, bubbleScale) {
  const petW = petPixelW(petScale);
  const petH = petPixelH(petScale);
  const bubbleW = Math.round(BUBBLE_MAX_W_BASE * bubbleScale + WINDOW_HORIZONTAL_PADDING);
  const w = Math.max(BASE_WINDOW_W, bubbleW, petW + WINDOW_HORIZONTAL_PADDING);
  const h = petH + Math.round(BUBBLE_AREA_H_BASE * bubbleScale);
  return { w, h };
}

// Apply pet-scale + bubble-scale: set the CSS custom property so
// .bubble paddings/font sizes scale, push the new pet sprite size down
// to Rust (so the click-through hit-test stays aligned), and resize the
// Tauri window so everything still fits.
//
// To keep the pet visually anchored when the user cycles scale at runtime,
// we shift the window so the bottom-center stays put — the pet sits at the
// bottom of the window, so "anchor bottom-center" == "pet doesn't jump".
async function applyWindowSize(petScale, bubbleScale) {
  document.body.style.setProperty('--bubble-scale', String(bubbleScale));
  // A unitless 1.4 line-height produced fractional 16.8px lines at the
  // default scale while the scroll viewport was 64px tall. Paging by that
  // viewport landed between glyph rows. Round the line box once, then make
  // each page exactly four whole lines at every supported scale.
  const { lineHeight, pageHeight } = bubblePageLayout(bubbleScale);
  document.body.style.setProperty('--bubble-line-height', `${lineHeight}px`);
  document.body.style.setProperty(
    '--bubble-page-height',
    `${pageHeight}px`,
  );
  const { w, h } = computeWindowSize(petScale, bubbleScale);
  try {
    const tauriCore = await import('@tauri-apps/api/core');
    // Tell Rust the new logical pet rect for the click-through hit-test.
    await tauriCore.invoke('set_pet_size', {
      w: petPixelW(petScale),
      h: petPixelH(petScale),
    });
  } catch {
    // browser mode — no-op
  }
  await resizeWindowBottomCenter(w, h);
}

// Resize without moving the pet: the stage is pinned to the bottom-center, so
// shifting the window by the inverse size delta keeps that point stationary.
async function resizeWindowBottomCenter(w, h) {
  try {
    const w_api = await import('@tauri-apps/api/window');
    const dpi = await import('@tauri-apps/api/dpi');
    const win = w_api.getCurrentWindow();
    const sf = await win.scaleFactor();
    const oldSize = await win.outerSize();
    const oldPos = await win.outerPosition();
    // outerSize/outerPosition are physical pixels; convert desired dimensions
    // before deciding whether a platform resize is needed.
    const newWPhys = Math.round(w * sf);
    const newHPhys = Math.round(h * sf);
    const dyPhys = newHPhys - oldSize.height;
    const dxPhys = newWPhys - oldSize.width;
    if (dyPhys === 0 && dxPhys === 0) return;
    await win.setSize(new dpi.LogicalSize(w, h));
    await win.setPosition(
      new dpi.PhysicalPosition(
        oldPos.x - Math.round(dxPhys / 2),
        oldPos.y - dyPhys,
      ),
    );
  } catch {
    // browser dev mode — nothing to resize.
  }
}

// The scale-derived dimensions are only a minimum: thread labels and multiple
// bubbles can make the rendered stack taller. Measure the complete stage so
// the transparent Tauri window never clips its top edge.
async function fitWindowToStage(stage, petScale, bubbleScale) {
  if (!stage) return;
  const measured = stage.getBoundingClientRect();
  const size = fitWindowSize(
    computeWindowSize(petScale, bubbleScale),
    measured,
  );
  await resizeWindowBottomCenter(size.w, size.h);
}

async function loadFromServer(serverUrl, name) {
  const base = `${serverUrl.replace(/\/$/, '')}/api/pet/asset/${name}`;
  const [meta, image] = await Promise.all([
    fetch(`${base}/pet.json`).then((r) => {
      if (!r.ok) throw new Error(`pet.json HTTP ${r.status}`);
      return r.json();
    }),
    loadImage(`${base}/spritesheet.webp`),
  ]);
  return { meta, image, source: 'server' };
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])
  );
}

// ---------- Help overlay ----------
//
// Shown via:
//  - First launch (one-time, persisted to localStorage so returning users
//    don't see it every time)
//  - `h` or `?` keys
//  - macOS menu: xangi-pets → Show Help (CmdOrCtrl+/) — emitted as Tauri
//    event `menu:help` from src-tauri/src/lib.rs
//
// Dismissed via Esc, click on the backdrop, or the close button. Uses
// capture-phase keydown so Esc gets handled even when other listeners are
// attached (e.g. the picker overlays).

const HELP_SHOWN_KEY = `${STORAGE_PREFIX}:help-shown`;

function readHelpSeen() {
  return globalThis.localStorage?.getItem(HELP_SHOWN_KEY) === '1';
}

function markHelpSeen() {
  globalThis.localStorage?.setItem(HELP_SHOWN_KEY, '1');
}

// Subscribe to Tauri events emitted from the Rust side (e.g. from menu
// items). Returns a no-op unlisten when running in browser dev mode where
// `@tauri-apps/api/event` isn't available.
async function tauriListen(name, handler) {
  try {
    const ev = await import('@tauri-apps/api/event');
    return ev.listen(name, handler);
  } catch {
    return () => {};
  }
}

let helpEscHandler = null;

// Snapshot of state shown in the help overlay's "current state" section.
// Mutated as the user changes pet / xangi URL so the next time they open
// Help they see the up-to-date values.
const helpState = {
  serverUrl: '',
  xangiUrl: null,
  petName: null,
  connection: 'not-configured',
  notificationsEnabled: false,
  normalResponsesEnabled: true,
  completionDisplayEnabled: true,
};

function isHelpOpen() {
  return !!document.getElementById('help-overlay');
}

function showHelp() {
  if (isHelpOpen()) return;
  const overlay = document.createElement('div');
  overlay.id = 'help-overlay';
  overlay.innerHTML = `
    <div class="help-card" role="dialog" aria-label="xangi-pets ヘルプ">
      <h2>xangi-pets キーボードショートカット</h2>
      <div class="help-section">
        <h3>操作</h3>
        <dl class="help-keys">
          <dt>x</dt><dd>xangi の URL を設定（pull 元の接続先）</dd>
          <dt>t</dt><dd>xangi にテキストを送信（ペットをクリックでも開く）</dd>
          <dt>c</dt><dd>ペット（スプライト）を切り替え</dd>
          <dt>p</dt><dd>ペットのサイズを循環</dd>
          <dt>b</dt><dd>ふきだしのサイズを循環</dd>
          <dt>1〜9</dt><dd>アニメ行を切り替え（開発用）</dd>
          <dt>h / ?</dt><dd>このヘルプを開く / 閉じる</dd>
          <dt>Esc</dt><dd>ヘルプ・モーダルを閉じる</dd>
        </dl>
      </div>
      <div class="help-section">
        <h3>いまの状態</h3>
        <div class="help-status" id="help-status-body"></div>
      </div>
      <div class="help-footer">
        <span>メニューバー <kbd>xangi-pets</kbd> → <kbd>Show Help</kbd> でも開けます（<kbd>⌘ /</kbd>）</span>
        <button type="button" class="help-close" aria-label="閉じる">閉じる</button>
      </div>
    </div>
  `;
  // Click outside the card (= on the backdrop) closes the overlay.
  overlay.addEventListener('click', (ev) => {
    if (ev.target === overlay) hideHelp();
  });
  overlay.querySelector('.help-close')?.addEventListener('click', hideHelp);

  document.body.appendChild(overlay);
  renderHelpState();
  // Force the window to accept clicks anywhere so the close button + footer
  // links work — without this the pet's hit-test polling makes the overlay
  // entirely click-through.
  pushModalClickGate();

  helpEscHandler = (ev) => {
    if (ev.key === 'Escape') {
      hideHelp();
      ev.stopPropagation();
    }
  };
  // Capture phase so Esc beats any picker-overlay handler that might also
  // be active (the help overlay sits on top so it should win).
  window.addEventListener('keydown', helpEscHandler, true);

  markHelpSeen();
}

function hideHelp() {
  const overlay = document.getElementById('help-overlay');
  if (!overlay) return;
  if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
  if (helpEscHandler) {
    window.removeEventListener('keydown', helpEscHandler, true);
    helpEscHandler = null;
  }
  popModalClickGate();
}

// Re-render only the "current state" section. Cheap enough that we just
// regenerate the DOM whenever state changes; keeps the rest of the overlay
// markup as a single template literal above.
function renderHelpState() {
  const body = document.getElementById('help-status-body');
  if (!body) return;
  const rows = [
    helpRow(
      'xangi URL',
      helpState.xangiUrl,
      helpState.xangiUrl ? 'help-ok' : 'help-warn',
      '未設定（x キーで入力）',
    ),
    helpRow(
      '接続',
      {
        'not-configured': '未設定',
        connecting: '接続中',
        connected: '接続済み',
        reconnecting: '再接続中',
        disconnected: '切断',
      }[helpState.connection] ?? helpState.connection,
      helpState.connection === 'connected' ? 'help-ok' : 'help-warn',
      '未設定',
    ),
    helpRow('通常応答', helpState.normalResponsesEnabled ? 'ON' : 'OFF', '', 'ON'),
    helpRow('完了通知', helpState.completionDisplayEnabled ? 'ON' : 'OFF', '', 'ON'),
    helpRow('システム通知', helpState.notificationsEnabled ? 'ON' : 'OFF', '', 'OFF'),
    helpRow('ペット', helpState.petName, '', '未選択'),
    helpRow('サーバ', helpState.serverUrl || '', '', '-'),
  ];
  body.innerHTML = rows.join('');
}

function helpRow(label, value, valueClass, placeholder) {
  const text = value && String(value).length > 0 ? String(value) : placeholder;
  const cls = ['help-value'];
  if (valueClass) cls.push(valueClass);
  return `
    <div class="help-row">
      <span class="help-key">${escapeHtml(label)}</span>
      <span class="${cls.join(' ')}">${escapeHtml(text)}</span>
    </div>
  `;
}

function updateHelpState(patch) {
  Object.assign(helpState, patch);
  if (isHelpOpen()) renderHelpState();
}

// Public-facing wrapper used by the menu listener and the first-launch
// auto-show. Pass the current state snapshot so the "いまの状態" section
// reflects whatever the user just changed before opening Help.
function showHelpOverlay(ctx) {
  if (ctx) updateHelpState(ctx);
  showHelp();
}

// In-window modal for setting the xangi URL. Replaces window.prompt() which
// is blocked by Tauri 2's WKWebView (silently no-ops; the user thinks the
// `x` key is broken). Returns one of:
//   { action: 'connect', url: <trimmed string> }
//   { action: 'disconnect' }
//   { action: 'cancel' }
//
// Same picker-overlay CSS as pickPet so the look stays
// consistent. Esc/backdrop-click/Cancel button all resolve as 'cancel'.
async function pickXangiUrl(currentUrl) {
  const initial = currentUrl || 'http://localhost:18888';
  const hasCurrent = !!currentUrl;

  // Grow the pet window so the input is comfortable to type / paste into,
  // and so the Cmd+drag text-selection has something bigger than 280px to
  // work with. Stash the old size/pos and restore them in cleanup().
  const sizeRestore = await growWindowForModal(460, 320);
  // Force-accept clicks for the duration of the modal — the pet's
  // click-through polling would otherwise eat clicks on the input field.
  await pushModalClickGate();

  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.id = 'xangi-url-modal';
    overlay.innerHTML = `
      <div class="picker-card xangi-url-card">
        <strong>xangi の接続先</strong>
        <p class="picker-hint">xangi 本体の URL（pull 元）。<kbd>Esc</kbd> でキャンセル</p>
        <input type="url" class="xangi-url-input" value="${escapeHtml(initial)}" placeholder="http://localhost:18888" spellcheck="false" autocomplete="off" />
        <div class="xangi-url-actions">
          <button type="button" class="picker-btn primary" data-action="connect">接続</button>
          ${
            hasCurrent
              ? '<button type="button" class="picker-btn ghost" data-action="disconnect">接続解除</button>'
              : ''
          }
          <button type="button" class="picker-btn ghost" data-action="cancel">キャンセル</button>
        </div>
      </div>
    `;
    const input = overlay.querySelector('.xangi-url-input');

    let resolved = false;
    async function cleanup(result) {
      if (resolved) return;
      resolved = true;
      window.removeEventListener('keydown', onKey, true);
      if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
      await popModalClickGate();
      await sizeRestore?.();
      resolve(result);
    }
    function onKey(ev) {
      if (ev.key === 'Escape') {
        ev.stopPropagation();
        cleanup({ action: 'cancel' });
        return;
      }
      if (ev.key === 'Enter' && document.activeElement === input) {
        ev.preventDefault();
        ev.stopPropagation();
        cleanup({ action: 'connect', url: input.value.trim() });
      }
    }

    overlay.addEventListener('click', (ev) => {
      // Backdrop click closes
      if (ev.target === overlay) {
        cleanup({ action: 'cancel' });
        return;
      }
      const action = ev.target?.dataset?.action;
      if (action === 'connect') cleanup({ action: 'connect', url: input.value.trim() });
      else if (action === 'disconnect') cleanup({ action: 'disconnect' });
      else if (action === 'cancel') cleanup({ action: 'cancel' });
    });

    window.addEventListener('keydown', onKey, true);
    document.body.appendChild(overlay);
    // Focus + select after the element is in the DOM so the user can type
    // immediately (matches the muscle memory of the old window.prompt).
    setTimeout(() => {
      input.focus();
      input.select();
    }, 0);
  });
}

// Grow the Tauri window to at least `targetW × targetH` (logical px) and
// return a function that restores the previous outer size / position. The
// pet sits at the bottom-center of the window, so we shift the window so
// that bottom-center stays anchored — same trick applyWindowSize() uses for
// the b/p key cycles. Returns null when running in browser dev mode (no
// Tauri APIs); callers treat that as a no-op.
async function growWindowForModal(targetW, targetH) {
  try {
    const w_api = await import('@tauri-apps/api/window');
    const dpi = await import('@tauri-apps/api/dpi');
    const win = w_api.getCurrentWindow();
    const sf = await win.scaleFactor();
    const oldSize = await win.outerSize();
    const oldPos = await win.outerPosition();

    const newW_phys = Math.round(targetW * sf);
    const newH_phys = Math.round(targetH * sf);
    const dxPhys = newW_phys - oldSize.width;
    const dyPhys = newH_phys - oldSize.height;

    await win.setSize(new dpi.LogicalSize(targetW, targetH));
    if (dxPhys !== 0 || dyPhys !== 0) {
      await win.setPosition(
        new dpi.PhysicalPosition(
          oldPos.x - Math.round(dxPhys / 2),
          oldPos.y - dyPhys,
        ),
      );
    }

    return async () => {
      try {
        await win.setSize(new dpi.PhysicalSize(oldSize.width, oldSize.height));
        await win.setPosition(new dpi.PhysicalPosition(oldPos.x, oldPos.y));
      } catch (err) {
        console.warn('xangi-pets: window restore failed', err);
      }
    };
  } catch {
    return null;
  }
}

// Show the xangi-URL modal and push the result to the Rust pull client.
// Used by the `x` key, the macOS menu's Preferences item, and the
// first-launch onboarding flow.
async function runSetXangiUrlPrompt(bubbleUi) {
  const cur = readXangiUrl();
  const result = await pickXangiUrl(cur);
  if (!result || result.action === 'cancel') return;

  if (result.action === 'disconnect') {
    try {
      await tauriInvoke('clear_xangi_url');
    } catch (err) {
      console.warn(`clear_xangi_url failed: ${err}`);
    }
    writeXangiUrl('');
    updateHelpState({ xangiUrl: null });
    bubbleUi?.showPreview?.('xangi 接続を解除しました');
    return;
  }

  // action === 'connect'
  const trimmed = result.url;
  if (!trimmed) return;
  try {
    const applied = await tauriInvoke('set_xangi_url', { url: trimmed });
    const value = typeof applied === 'string' ? applied : trimmed;
    writeXangiUrl(value);
    updateHelpState({ xangiUrl: value });
    bubbleUi?.showPreview?.(`xangi URL: ${value}`);
    console.info(`xangi-pets: xangi URL -> ${value}`);
  } catch (err) {
    console.error(`xangi-pets: set_xangi_url failed: ${err}`);
    bubbleUi?.showPreview?.(`接続失敗: ${err}`);
  }
}

// In-window modal for sending a single text line to the upstream xangi via
// `POST /api/pet/inbox`. Used by the `t` key and by a short click on the
// pet sprite (long press / drag still moves the window).
//
// Returns one of:
//   { action: 'send', text: <trimmed string> }
//   { action: 'cancel' }
async function pickPetMessage() {
  const sizeRestore = await growWindowForModal(460, 240);
  await pushModalClickGate();

  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.id = 'pet-message-modal';
    overlay.innerHTML = `
      <div class="picker-card xangi-url-card">
        <strong>xangi に話しかける</strong>
        <p class="picker-hint">1 行入力して <kbd>Enter</kbd> で送信。<kbd>Esc</kbd> でキャンセル</p>
        <input type="text" class="xangi-url-input" placeholder="今日の天気は？" spellcheck="false" autocomplete="off" />
        <div class="xangi-url-actions">
          <button type="button" class="picker-btn primary" data-action="send">送信</button>
          <button type="button" class="picker-btn ghost" data-action="cancel">キャンセル</button>
        </div>
      </div>
    `;
    const input = overlay.querySelector('.xangi-url-input');

    let resolved = false;
    async function cleanup(result) {
      if (resolved) return;
      resolved = true;
      window.removeEventListener('keydown', onKey, true);
      if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
      await popModalClickGate();
      await sizeRestore?.();
      resolve(result);
    }
    function onKey(ev) {
      if (ev.key === 'Escape') {
        ev.stopPropagation();
        cleanup({ action: 'cancel' });
        return;
      }
      if (ev.key === 'Enter' && document.activeElement === input) {
        // 日本語入力 (IME) の漢字変換確定 Enter で送信されないようにする。
        // - ev.isComposing: 標準 (Chromium / WebKit / Firefox)
        // - keyCode 229: 古いブラウザの IME-composing 用フォールバック
        // どちらか true なら確定処理として扱い、送信はしない。
        if (ev.isComposing || ev.keyCode === 229) return;
        ev.preventDefault();
        ev.stopPropagation();
        cleanup({ action: 'send', text: input.value.trim() });
      }
    }

    overlay.addEventListener('click', (ev) => {
      if (ev.target === overlay) {
        cleanup({ action: 'cancel' });
        return;
      }
      const action = ev.target?.dataset?.action;
      if (action === 'send') cleanup({ action: 'send', text: input.value.trim() });
      else if (action === 'cancel') cleanup({ action: 'cancel' });
    });

    window.addEventListener('keydown', onKey, true);
    document.body.appendChild(overlay);
    setTimeout(() => {
      input.focus();
    }, 0);
  });
}

// Show the pet-message modal and POST the result through the Rust
// `send_pet_message` command. Called by the `t` key and by short-click on
// the pet sprite.
async function runPetMessagePrompt(bubbleUi) {
  // 2 重起動防止 (modal が開いてる間に pet を連打すると DOM に複数
  // overlay が並んでしまう、2026-05-28 報告)。
  if (document.querySelector('#pet-message-modal')) return;
  const cur = await tauriInvoke('get_xangi_url').catch(() => null);
  if (!cur) {
    bubbleUi?.showPreview?.('xangi URL 未設定 — `x` で先に設定してね');
    return;
  }
  const result = await pickPetMessage();
  if (!result || result.action !== 'send') return;
  const text = result.text;
  if (!text) return;
  try {
    await tauriInvoke('send_pet_message', { text });
    bubbleUi?.showPreview?.(`> ${text.slice(0, 60)}`);
    console.info(`xangi-pets: sent pet message (${text.length} chars)`);
  } catch (err) {
    console.error(`xangi-pets: send_pet_message failed: ${err}`);
    bubbleUi?.showPreview?.(`送信失敗: ${err}`);
  }
}

// Fetch the list of available pets from the embedded server and let the
// user click one. Resolves with the chosen name, or null if no pets exist.
async function pickPet(serverUrl, currentName) {
  const r = await fetch(`${serverUrl.replace(/\/$/, '')}/api/pet/list`);
  if (!r.ok) throw new Error(`pet list HTTP ${r.status}`);
  const { pets } = await r.json();
  if (!Array.isArray(pets) || pets.length === 0) return null;

  // Same window-grow + click-gate dance as pickXangiUrl. Without this the
  // pet rows are click-through and selection is impossible.
  const sizeRestore = await growWindowForModal(360, 380);
  await pushModalClickGate();

  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.id = 'pet-picker';
    overlay.innerHTML = `
      <div class="picker-card">
        <strong>Choose a pet</strong>
        <p class="picker-hint">Click to select. Press <kbd>Esc</kbd> to cancel.</p>
        <div class="picker-list"></div>
      </div>
    `;
    const list = overlay.querySelector('.picker-list');
    for (const name of pets) {
      const btn = document.createElement('button');
      btn.className = 'picker-item';
      btn.textContent = name;
      if (name === currentName) {
        btn.classList.add('current');
      }
      btn.addEventListener('click', () => cleanup(name));
      list.appendChild(btn);
    }

    function onKey(ev) {
      if (ev.key === 'Escape') cleanup(null);
    }
    let resolved = false;
    async function cleanup(chosen) {
      if (resolved) return;
      resolved = true;
      window.removeEventListener('keydown', onKey, true);
      if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
      await popModalClickGate();
      await sizeRestore?.();
      resolve(chosen);
    }
    window.addEventListener('keydown', onKey, true);
    document.body.appendChild(overlay);
  });
}

// Show a small in-window hint explaining where to drop the pet sprite.
// Triggered when /api/pet/asset/<name>/{pet.json,spritesheet.webp} can't be
// loaded from the embedded server (i.e. user hasn't set up assets yet).
function showSetupHint(serverUrl, petName, err) {
  const stage = document.getElementById('stage');
  if (!stage) return;
  const canvas = document.getElementById('pet');
  if (canvas) canvas.style.display = 'none';
  const bubble = document.getElementById('bubble');
  if (bubble) bubble.hidden = true;

  const hint = document.createElement('div');
  hint.id = 'setup-hint';
  hint.innerHTML = `
    <strong>No pet sprite for "${petName}"</strong>
    <p>Place <code>pet.json</code> + <code>spritesheet.webp</code> at one of:</p>
    <pre>~/.xangi/pets/${petName}/
~/.codex/pets/${petName}/</pre>
    <p>See <a href="https://github.com/karaage0703/xangi-pets#セットアップ" target="_blank">README</a> for the sprite-sheet spec.</p>
    <p class="muted">server: ${serverUrl}</p>
  `;
  stage.appendChild(hint);
  document.title = `xangi-pets · setup needed`;
  console.error('xangi-pets setup hint:', err);
}

function loadImage(src) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    // The embedded server runs on a different port (7895) than the Vite dev
    // server (1420), so the spritesheet is cross-origin. Without this flag
    // the canvas becomes "tainted" and detectFilledFrames' getImageData()
    // throws SecurityError. The server returns Access-Control-Allow-Origin:*
    // so anonymous CORS is enough.
    img.crossOrigin = 'anonymous';
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`image load failed: ${src}`));
    img.src = src;
  });
}

// hatch-pet leaves trailing cells fully transparent for rows that need fewer
// than 8 frames. Walk the spritesheet at load and remember which columns are
// actually filled per row, otherwise the loop draws blank cells and flickers.
function detectFilledFrames(image) {
  const off = document.createElement('canvas');
  off.width = image.width;
  off.height = image.height;
  const ctx = off.getContext('2d', { willReadFrequently: true });
  ctx.drawImage(image, 0, 0);

  const out = [];
  for (let r = 0; r < ROWS; r++) {
    const filled = [];
    for (let c = 0; c < COLS; c++) {
      const { data } = ctx.getImageData(c * CELL_W, r * CELL_H, CELL_W, CELL_H);
      for (let i = 3; i < data.length; i += 4) {
        if (data[i] > 5) {
          filled.push(c);
          break;
        }
      }
    }
    out.push(filled.length > 0 ? filled : [0]);
  }
  return out;
}

function makeRenderer(canvas, image, drawW, drawH) {
  let _drawW = drawW;
  let _drawH = drawH;
  canvas.width = _drawW;
  canvas.height = _drawH;
  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingEnabled = false;

  const filledFrames = detectFilledFrames(image);
  let row = 0;
  let step = 0;
  // Last xangi state we were told about. The wandering loop checks this so
  // it only animates while the pet is genuinely idle (not while xangi is
  // thinking/talking/erroring).
  let currentState = 'idle';

  function currentColumn() {
    const cols = filledFrames[row];
    return cols[step % cols.length];
  }

  function draw() {
    ctx.clearRect(0, 0, _drawW, _drawH);
    const sx = currentColumn() * CELL_W;
    const sy = row * CELL_H;
    ctx.drawImage(image, sx, sy, CELL_W, CELL_H, 0, 0, _drawW, _drawH);
  }

  // In-place sprite resize. Setting canvas.width/height resets the 2d context
  // (image-smoothing flips back to default true), so reapply that and redraw.
  // detectFilledFrames runs against the *source image* not the display canvas,
  // so its result stays valid across resize — no need to recompute.
  function setSize(w, h) {
    _drawW = w;
    _drawH = h;
    canvas.width = w;
    canvas.height = h;
    ctx.imageSmoothingEnabled = false;
    draw();
  }

  function setRow(name) {
    const idx = ROW_NAMES.indexOf(name);
    if (idx >= 0 && idx < ROWS) {
      row = idx;
      step = 0;
    }
  }

  function setState(state) {
    currentState = state;
    const rowName = STATE_TO_ROW[state] ?? 'idle';
    setRow(rowName);
  }

  function getState() {
    return currentState;
  }

  function tick() {
    step += 1;
    draw();
  }

  return { draw, tick, setRow, setState, getState, setSize };
}

// Idle wandering: every few seconds, with some probability, nudge the
// window left or right by a small amount so the pet looks alive instead of
// frozen. Only fires while xangi state is `idle` — if the user is actively
// chatting we let the state-driven animation own the sprite. The sprite row
// briefly switches to running-* during the move; the state SSE will overwrite
// it back if state changes mid-wander.
function startWandering(renderer) {
  let busy = false;

  async function wanderOnce() {
    if (busy) return;
    if (renderer.getState() !== 'idle') return;
    busy = true;
    try {
      const w = await import('@tauri-apps/api/window');
      const dpi = await import('@tauri-apps/api/dpi');
      const win = w.getCurrentWindow();

      const dir = Math.random() < 0.5 ? -1 : 1;
      // 30〜90 physical px — visible but not jarring.
      const distance = Math.round((30 + Math.random() * 60) * dir);
      const steps = 18;
      const stepPx = distance / steps;
      const dt = 35; // ms per step → ~0.6s total animation

      renderer.setRow(dir < 0 ? 'running-left' : 'running-right');

      const start = await win.outerPosition();
      for (let i = 1; i <= steps; i++) {
        // Bail out if a real state arrived (talking/thinking/error) — don't
        // fight the SSE-driven animation.
        if (renderer.getState() !== 'idle') break;
        const x = Math.round(start.x + stepPx * i);
        await win.setPosition(new dpi.PhysicalPosition(x, start.y));
        await new Promise((r) => setTimeout(r, dt));
      }
    } catch (err) {
      console.warn('xangi-pets: wander failed', err);
    } finally {
      // Restore idle row only if SSE hasn't moved us elsewhere mid-wander.
      if (renderer.getState() === 'idle') renderer.setRow('idle');
      busy = false;
    }
  }

  function scheduleNext() {
    // 8〜25s between attempts; 50% of attempts actually move (so the pet
    // spends most of its time still, with the occasional twitch).
    const delay = 8000 + Math.random() * 17000;
    setTimeout(async () => {
      if (Math.random() < 0.5) await wanderOnce();
      scheduleNext();
    }, delay);
  }

  scheduleNext();
}

// Tell the Rust side how many bubbles are currently visible. Rust uses this
// to decide whether the entire window should accept mouse clicks (so the
// user can dismiss bubbles by clicking) or only the pet sprite rectangle.
async function notifyBubbleActive(active) {
  try {
    const tauri = await import('@tauri-apps/api/core');
    await tauri.invoke('set_bubble_active', { active });
  } catch (err) {
    // browser mode (no Tauri shell) — fine, click-through doesn't apply.
  }
}

// Modal click gate. The pet window is normally click-through outside the
// pet sprite (50ms hit-test polling in src-tauri/src/lib.rs flips
// set_ignore_cursor_events based on cursor position). That breaks any UI
// where the user has to interact with controls anywhere else in the window
// — help overlay buttons, the xangi URL modal's input, mouse-drag to select
// text, paste, etc. Each modal calls pushModalClickGate() on open and
// popModalClickGate() on close; while the depth is positive the entire
// window receives clicks (we reuse Rust's `set_bubble_active` flag because
// it has the same semantics: "accept clicks anywhere right now").
const clickGate = makeClickGateController(notifyBubbleActive);
async function pushModalClickGate() {
  await clickGate.pushModal();
}
async function popModalClickGate() {
  await clickGate.popModal();
}

function subscribeState(serverUrl, renderer) {
  const url = `${serverUrl.replace(/\/$/, '')}/api/pet/state`;
  let es = new EventSource(url);
  es.onmessage = (ev) => {
    try {
      const payload = JSON.parse(ev.data);
      if (payload?.state) renderer.setState(payload.state);
    } catch (err) {
      console.warn('xangi-pets: bad SSE payload', ev.data, err);
    }
  };
  es.onerror = () => {
    // EventSource auto-reconnects; just log.
    console.debug('xangi-pets: SSE disconnected, retrying');
  };
  return es;
}


async function main() {
  const canvas = document.getElementById('pet');

  // Resolve the server URL first — the embedded HTTP server may have
  // auto-shifted off the default port, so we ask Rust for the actual one.
  const serverUrl = await resolveServerUrl();
  console.info(`xangi-pets: using server ${serverUrl}`);

  // Now that we know which port we're bound to, derive a per-instance
  // localStorage namespace. Two `open -n` instances will have different
  // ports and thus separate pet/bubble-scale settings; a single instance
  // still inherits the legacy unprefixed values via readStorage's fallback.
  storageNamespace = deriveNamespace(serverUrl);
  console.info(`xangi-pets: storage namespace = ${storageNamespace || '(none)'}`);
  const notificationsEnabled = readStorage('notifications') === '1';
  const normalResponsesEnabled = readStorage('normal-responses') !== '0';
  const completionDisplayEnabled = readStorage('completion-display') !== '0';
  await tauriInvoke('set_notifications_enabled', { enabled: notificationsEnabled }).catch((err) => {
    console.warn('xangi-pets: notification preference could not be applied', err);
  });
  await tauriInvoke('set_normal_responses_enabled', { enabled: normalResponsesEnabled }).catch(
    (err) => console.warn('xangi-pets: normal response preference could not be applied', err),
  );
  await tauriInvoke('set_completion_display_enabled', {
    enabled: completionDisplayEnabled,
  }).catch((err) =>
    console.warn('xangi-pets: completion display preference could not be applied', err),
  );

  // Subscribe only after restoring the notification preference. That closes
  // the startup race where a new turn could begin between the SSE handshake
  // and applying a persisted "notifications on" setting.
  const xangiUrl = await ensureXangiUrl();
  if (xangiUrl) {
    console.info(`xangi-pets: subscribing to xangi at ${xangiUrl}/api/events/stream`);
  } else {
    console.warn('xangi-pets: no xangi URL configured — bubbles disabled. Press `x` to set.');
  }

  // Sprite scale and bubble scale are mutable at runtime via the `p` and `b`
  // keys, so keep them in `let` bindings the keydown handler can mutate.
  let spriteScale = readScale();
  let bubbleScale = readBubbleScale();
  let drawW = petPixelW(spriteScale);
  let drawH = petPixelH(spriteScale);

  // Bubble + pet scale together drive window size, CSS variable, and Rust's
  // hit-test rectangle. Apply early so the window is the right size before
  // bubbles arrive.
  await applyWindowSize(spriteScale, bubbleScale);

  // Resolve which pet to show. localStorage wins; otherwise prompt the
  // user with the picker. If they have no sprites at all, fall through
  // to the setup hint.
  let petName = readPetName();
  if (!petName) {
    try {
      petName = await pickPet(serverUrl, null);
    } catch (err) {
      console.warn('xangi-pets: pet list failed', err);
    }
    if (!petName) {
      showSetupHint(serverUrl, '?', new Error('no pets configured'));
      return;
    }
    writeStorage('name', petName);
  }

  let pet;
  try {
    pet = await loadFromServer(serverUrl, petName);
  } catch (err) {
    console.warn('xangi-pets: sprite load failed', err);
    showSetupHint(serverUrl, petName, err);
    return;
  }

  document.title = `xangi-pets · ${pet.meta.displayName ?? pet.meta.id ?? petName} (${pet.source})`;

  const renderer = makeRenderer(canvas, pet.image, drawW, drawH);
  renderer.setRow('idle');
  renderer.draw();

  setInterval(() => renderer.tick(), 1000 / FPS);

  subscribeState(serverUrl, renderer);
  startWandering(renderer);
  const stage = document.getElementById('stage');
  let stageFitScheduled = false;
  function scheduleStageWindowFit() {
    if (!stage || stageFitScheduled) return;
    stageFitScheduled = true;
    requestAnimationFrame(() => {
      stageFitScheduled = false;
      void fitWindowToStage(stage, spriteScale, bubbleScale);
    });
  }
  const bubbleUi = makeBubbleUI({
    root: document.getElementById('bubbles'),
    // Keep the stack compact: older bubbles are evicted when a 3rd arrives.
    // The stage fitter below handles the rendered height of the remaining
    // bubbles, including thread labels and streamed text.
    maxBubbles: 2,
    normalResponsesEnabled,
    completionDisplayEnabled,
    onCountChange: (count) => {
      // Tell Rust whether at least one bubble is on screen. Rust uses this
      // to decide whether the whole window should accept clicks (so the user
      // can dismiss bubbles) or only the pet sprite rectangle.
      void clickGate.setBubbleCount(count);
      scheduleStageWindowFit();
    },
  });

  subscribeBubbles(serverUrl, bubbleUi);
  window.__bubble = bubbleUi;

  // Bubble text grows while SSE deltas stream in, and dismiss/eviction shrinks
  // the stack. Follow the rendered stage in both directions.
  if (typeof ResizeObserver !== 'undefined' && stage) {
    new ResizeObserver(scheduleStageWindowFit).observe(stage);
  }
  scheduleStageWindowFit();

  // -webkit-app-region: drag does not work on Tauri's frameless macOS window;
  // call startDragging() from mousedown instead. Attach to the canvas only,
  // not the whole document — the surrounding transparent area is now
  // click-through (see src-tauri/src/lib.rs).
  //
  // Short-tap detection: if mousedown→mouseup happens without measurable
  // cursor movement, treat it as a click instead of a drag and open the
  // talk-to-xangi modal. The threshold (5px) avoids accidental opens when
  // the user nudges the pet a couple of pixels. startDragging() is still
  // called on mousedown so an actual drag works as before; on a short tap
  // the OS just hasn't moved the window because the cursor didn't move.
  const CLICK_DRAG_THRESHOLD_PX = 5;
  let downPos = null;
  canvas.addEventListener('mousedown', async (ev) => {
    if (ev.button !== 0) return;
    downPos = { x: ev.screenX, y: ev.screenY };
    try {
      const w = await import('@tauri-apps/api/window');
      await w.getCurrentWindow().startDragging();
    } catch (err) {
      console.warn('startDragging unavailable (browser mode?)', err);
    }
  });
  canvas.addEventListener('mouseup', async (ev) => {
    if (ev.button !== 0 || !downPos) return;
    const dx = ev.screenX - downPos.x;
    const dy = ev.screenY - downPos.y;
    downPos = null;
    if (Math.hypot(dx, dy) > CLICK_DRAG_THRESHOLD_PX) return;
    await runPetMessagePrompt(bubbleUi);
  });

  // Build the snapshot the help overlay needs. Captured at call time so the
  // displayed values track whatever the user just changed via key bindings.
  const helpContext = () => ({
    xangiUrl: readXangiUrl(),
    petName,
    serverUrl,
    petScale: spriteScale,
    bubbleScale,
    notificationsEnabled: readStorage('notifications') === '1',
    normalResponsesEnabled: readStorage('normal-responses') !== '0',
    completionDisplayEnabled: readStorage('completion-display') !== '0',
  });

  // Seed the help-overlay state immediately so a `pet://show-help` event
  // arriving before the user touches anything still has the live URLs / pet
  // name to display.
  updateHelpState(helpContext());

  // Surface help via the macOS menu bar and the system tray (set up in Rust).
  // Both emit `pet://show-help` (or `set-xangi-url` / `quit`) — we listen here
  // and dispatch to the same handlers the keys use.
  await tauriListen('pet://show-help', () => showHelpOverlay(helpContext()));
  await tauriListen('pet://set-xangi-url', async () => {
    await runSetXangiUrlPrompt(bubbleUi);
  });
  await tauriListen('pet://talk', async () => {
    await runPetMessagePrompt(bubbleUi);
  });
  await tauriListen('pet://connection-status', ({ payload }) => {
    updateHelpState({ connection: String(payload ?? 'disconnected') });
  });
  await tauriListen('pet://notifications-changed', ({ payload }) => {
    const enabled = payload === true;
    writeStorage('notifications', enabled ? '1' : '0');
    updateHelpState({ notificationsEnabled: enabled });
  });
  await tauriListen('pet://normal-responses-changed', ({ payload }) => {
    const enabled = payload === true;
    writeStorage('normal-responses', enabled ? '1' : '0');
    bubbleUi.setDisplayPreferences({ normalResponses: enabled });
    updateHelpState({ normalResponsesEnabled: enabled });
  });
  await tauriListen('pet://completion-display-changed', ({ payload }) => {
    const enabled = payload === true;
    writeStorage('completion-display', enabled ? '1' : '0');
    bubbleUi.setDisplayPreferences({ completions: enabled });
    updateHelpState({ completionDisplayEnabled: enabled });
  });
  const connection = await tauriInvoke('get_connection_status').catch(() => 'not-configured');
  updateHelpState({ connection: String(connection ?? 'not-configured') });
  await tauriListen('pet://reset-pet', async () => {
    let chosen;
    try {
      chosen = await pickPet(serverUrl, petName);
    } catch (err) {
      console.warn('xangi-pets: pet list failed', err);
      return;
    }
    if (chosen && chosen !== petName) {
      writeStorage('name', chosen);
      location.reload();
    }
  });

  // First-launch: auto-show help once the user has settled (xangi URL prompt
  // dismissed, pet sprite loaded). showHelp() persists HELP_SHOWN_KEY itself
  // via markHelpSeen() so subsequent launches skip this branch.
  if (!readHelpSeen()) {
    showHelpOverlay(helpContext());
  }

  // 1-9: cycle animation row (dev). x: set xangi (upstream) URL. c: change
  // pet. b: cycle bubble scale. p: cycle pet (sprite) scale. h / ?: toggle
  // help. t: talk to xangi. The old `s` key (override the embedded server
  // URL) was removed — it's a dev-only knob and the option confused end
  // users who thought they had to set two URLs. Use the XANGI_PET_PORT /
  // XANGI_PET_BIND env vars if you need to override.
  //
  // Suppress all key bindings while a modal overlay is open. Without this,
  // typing into the talk / URL input fires h / x / c / b / p / 1-9 etc. as
  // shortcuts and pops up the help overlay or the pet picker on top of the
  // modal. The modal's own Esc/Enter handlers run in capture phase and call
  // stopPropagation, so they still work; this just gates the shortcut layer
  // below them.
  window.addEventListener('keydown', async (ev) => {
    if (
      document.querySelector(
        '#pet-message-modal, #xangi-url-modal, #help-overlay, .picker-overlay'
      )
    ) {
      return;
    }
    const n = Number(ev.key);
    if (Number.isInteger(n) && n >= 1 && n <= ROWS) {
      renderer.setRow(ROW_NAMES[n - 1]);
      return;
    }
    if (ev.key === 'c') {
      let chosen;
      try {
        chosen = await pickPet(serverUrl, petName);
      } catch (err) {
        console.warn('xangi-pets: pet list failed', err);
        return;
      }
      if (chosen && chosen !== petName) {
        writeStorage('name', chosen);
        location.reload();
      }
    }
    if (ev.key === 'b') {
      // Cycle through preset bubble sizes in-place. Window resize + CSS var
      // are applied immediately; existing bubbles stay on screen.
      const idx = BUBBLE_SCALE_STEPS.findIndex((s) => Math.abs(s - bubbleScale) < 0.01);
      const next = BUBBLE_SCALE_STEPS[(idx + 1) % BUBBLE_SCALE_STEPS.length];
      bubbleScale = next;
      writeStorage('bubble-scale', String(next));
      await applyWindowSize(spriteScale, next);
      // Show a preview bubble so the size change is visible even when no
      // real bubble is currently on screen — also makes the resize step
      // less flickery because there's always something rendered in the
      // bubble area while the user is cycling sizes.
      bubbleUi.showPreview(`プレビュー: バブル ${next}x`);
      console.info(`xangi-pets: bubble-scale -> ${next}`);
    }
    if (ev.key === 'p') {
      // Cycle through preset pet (sprite) sizes in-place. We resize the
      // canvas + Tauri window rather than reloading; existing bubbles and
      // SSE streams keep running.
      const idx = PET_SCALE_STEPS.findIndex((s) => Math.abs(s - spriteScale) < 0.01);
      const next = PET_SCALE_STEPS[(idx + 1) % PET_SCALE_STEPS.length];
      spriteScale = next;
      drawW = petPixelW(next);
      drawH = petPixelH(next);
      writeStorage('scale', String(next));
      renderer.setSize(drawW, drawH);
      await applyWindowSize(next, bubbleScale);
      bubbleUi.showPreview(`プレビュー: ペット ${next}x`);
      console.info(`xangi-pets: pet-scale -> ${next}`);
    }
    if (ev.key === 'x') {
      await runSetXangiUrlPrompt(bubbleUi);
      return;
    }
    if (ev.key === 't') {
      // Talk to xangi: open a 1-line input modal and POST it to the upstream
      // /api/pet/inbox. Response arrives later as a normal SSE bubble.
      await runPetMessagePrompt(bubbleUi);
      return;
    }
    if (ev.key === 'h' || ev.key === '?') {
      // Toggle the help overlay. Two keys because `?` is the more discoverable
      // "what does this app do?" shortcut while `h` is the muscle-memory choice
      // for users used to vim-style help.
      if (isHelpOpen()) {
        hideHelp();
      } else {
        showHelpOverlay(helpContext());
      }
      return;
    }
    if (ev.key === 'Escape') {
      // The help overlay registers its own capture-phase Esc handler in
      // showHelp(), so this is just a fallback for picker-overlay edge cases.
      if (isHelpOpen()) {
        hideHelp();
        return;
      }
    }
  });

  window.__pet = renderer;
}

main().catch((err) => {
  console.error('xangi-pets failed to start:', err);
});
