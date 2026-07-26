// Speech-bubble UI driver for the pet.
//
// Each (thread_id, turn_id) gets its own bubble in a vertical stack above the
// pet sprite. The most recent bubble sits closest to the pet (i.e. at the
// bottom of the stack, since the body is flex-end justified). Bubbles do NOT
// fade out on close — they hang around until the user clicks them, so a quick
// "talk-and-leave" turn from another channel doesn't disappear before the
// human has a chance to read it.
//
// Cap is small (default 3) — when a 4th bubble arrives, the oldest is dropped
// silently.

const DEFAULT_MAX = 3;
const PREVIEW_AUTO_HIDE_MS = 4000;
const SCROLL_EPSILON_PX = 1;
const LINE_HEIGHT_BASE_PX = 16.8; // 12px font × the original 1.4 line-height
const DEFAULT_PAGE_LINES = 4;
const REPLY_SUGGESTIONS_START = '<xangi_reply_suggestions>';
const REPLY_SUGGESTIONS_PREFIX = '<xangi_';
// How long each "page" of an over-long bubble stays on screen before the
// bubble auto-scrolls to the next page. Loops back to the top after the
// last page so the user can re-read.
const PAGE_CYCLE_MS = 4000;

// Reply suggestions are internal xangi UI metadata. They arrive in the same
// streamed text as the answer, so hide a standalone marker line as soon as a
// sufficiently specific prefix appears. Inline prose that merely mentions the
// tag remains visible.
export function stripInternalReplySuggestions(text) {
  const value = String(text ?? '');
  const lines = value.split(/\r?\n/);
  for (let index = 0; index < lines.length; index++) {
    const candidate = lines[index].trim();
    const completeMarker = candidate.startsWith(REPLY_SUGGESTIONS_START);
    const streamingMarker =
      candidate.length >= REPLY_SUGGESTIONS_PREFIX.length &&
      REPLY_SUGGESTIONS_START.startsWith(candidate);
    if (completeMarker || streamingMarker) {
      return lines.slice(0, index).join('\n').trimEnd();
    }
  }
  return value;
}

export function bubblePageLayout(scale, pageLines = DEFAULT_PAGE_LINES) {
  const safeScale = Number.isFinite(scale) && scale > 0 ? scale : 1;
  const safePageLines =
    Number.isInteger(pageLines) && pageLines > 0 ? pageLines : DEFAULT_PAGE_LINES;
  const lineHeight = Math.round(LINE_HEIGHT_BASE_PX * safeScale);
  return {
    lineHeight,
    pageHeight: lineHeight * safePageLines,
  };
}

// Return the next page-aligned scroll position. The final page can be shorter
// than the viewport, so visit `max` once before looping back to the beginning.
// CSS keeps the viewport and line-height in whole-pixel, whole-line units;
// preserving the final `max` stop keeps the last lines visible without
// clipping their top or skipping them entirely.
export function nextPageScrollTop(scrollTop, scrollHeight, clientHeight) {
  const max = Math.max(0, scrollHeight - clientHeight);
  if (max <= SCROLL_EPSILON_PX) return 0;
  if (scrollTop >= max - SCROLL_EPSILON_PX) return 0;
  return Math.min(scrollTop + clientHeight, max);
}

export function makeBubbleUI({ root, maxBubbles = DEFAULT_MAX, onCountChange } = {}) {
  if (!root) throw new Error('makeBubbleUI: root is required');

  let lastNotifiedCount = 0;
  function notifyCount() {
    if (typeof onCountChange !== 'function') return;
    if (bubbles.length === lastNotifiedCount) return;
    lastNotifiedCount = bubbles.length;
    try {
      onCountChange(bubbles.length);
    } catch (err) {
      console.warn('xangi-pets: onCountChange threw', err);
    }
  }

  // Preview bubble used by the size-cycling key handlers (`b` / `p`). It's a
  // separate DOM node from the real bubble stack so it never enters `bubbles[]`
  // and never affects click-through (notifyCount stays based on real bubbles).
  let previewEl = null;
  let previewTimer = null;
  function showPreview(text) {
    if (!previewEl) {
      previewEl = document.createElement('div');
      previewEl.className = 'bubble preview';
      const threadEl = document.createElement('div');
      threadEl.className = 'bubble-thread';
      threadEl.textContent = '(プレビュー)';
      threadEl.hidden = false;
      const textEl = document.createElement('div');
      textEl.className = 'bubble-text';
      previewEl.appendChild(threadEl);
      previewEl.appendChild(textEl);
      previewEl._textEl = textEl;
      // Newest bubble sits at the bottom (closest to pet). Insert preview at
      // the bottom too so users see it where real bubbles would appear.
      root.appendChild(previewEl);
    }
    previewEl._textEl.textContent = text;
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(() => clearPreview(), PREVIEW_AUTO_HIDE_MS);
  }
  function clearPreview() {
    if (previewTimer) {
      clearTimeout(previewTimer);
      previewTimer = null;
    }
    if (previewEl) {
      if (previewEl.parentNode) previewEl.parentNode.removeChild(previewEl);
      previewEl = null;
    }
  }

  // Ordered oldest -> newest. Each entry tracks a single (thread, turn) bubble
  // and the DOM nodes that render it. We render newest-at-bottom by inserting
  // newer bubbles last and relying on CSS flex column ordering.
  const bubbles = []; // { id, thread_id, turn_id, text, status, el, textEl, threadEl }

  function bubbleId(thread_id, turn_id) {
    return `${thread_id}\u0000${turn_id}`;
  }

  function find(thread_id, turn_id) {
    const id = bubbleId(thread_id, turn_id);
    return bubbles.find((b) => b.id === id);
  }

  function shortThread(threadId) {
    if (typeof threadId !== 'string') return String(threadId);
    if (threadId.length <= 14) return threadId;
    return threadId.slice(0, 8) + '…' + threadId.slice(-6);
  }

  function displayLabel(b) {
    // Prefer the human-readable label sent by the publisher (e.g. Discord
    // channel name "#general"). Fall back to a shortened thread_id
    // when no label is available.
    return b.label && b.label.length > 0 ? b.label : shortThread(b.thread_id);
  }

  function renderThreadTags() {
    // Show the tag whenever we have a human-readable label (e.g. Discord
    // channel name) — even with a single bubble, "#general" is
    // useful context. When no label is available we fall back to the
    // multi-thread rule: show a shortened id only if more than one distinct
    // thread is on screen, otherwise hide (the truncated id is just noise).
    const distinctThreads = new Set(bubbles.map((b) => b.thread_id));
    const multipleThreads = distinctThreads.size > 1;
    for (const b of bubbles) {
      const hasLabel = typeof b.label === 'string' && b.label.length > 0;
      const show = hasLabel || multipleThreads;
      if (show) {
        b.threadEl.textContent = displayLabel(b);
        b.threadEl.hidden = false;
      } else {
        b.threadEl.hidden = true;
      }
    }
  }

  function clearPagingTimer(b) {
    if (b._cycleTimer) {
      clearInterval(b._cycleTimer);
      b._cycleTimer = null;
    }
  }

  // When a bubble's text exceeds its fixed CSS max-height, scroll the text
  // viewport one page at a time and loop back to the top. Streaming bubbles
  // (status='open') stick to the bottom so the user always sees the newest
  // delta; paging only kicks in once the bubble is closed/error.
  function schedulePaging(b) {
    clearPagingTimer(b);
    const el = b.textEl;
    // Smoke test fake DOM doesn't expose layout APIs — bail silently so the
    // existing tests keep passing without paging running.
    if (!el || typeof el.scrollHeight !== 'number') return;
    const raf = globalThis.requestAnimationFrame;
    const setI = globalThis.setInterval;
    if (typeof raf !== 'function' || typeof setI !== 'function') return;

    raf(() => {
      if (!el.parentNode) return; // bubble dismissed before rAF fired
      // Rapid applyText calls (open + N deltas + close) queue multiple rAFs
      // that all run before any timer fires. Clear any timer set by an
      // earlier rAF before we install a new one, otherwise intervals leak.
      clearPagingTimer(b);
      const overflow = el.scrollHeight - el.clientHeight;
      if (overflow <= 0) {
        el.scrollTop = 0;
        return;
      }
      if (b.status === 'open') {
        // Streaming: stick to the bottom so newest delta is visible.
        el.scrollTop = el.scrollHeight;
        return;
      }
      el.scrollTop = 0;
      b._cycleTimer = setI(() => {
        el.scrollTop = nextPageScrollTop(
          el.scrollTop,
          el.scrollHeight,
          el.clientHeight,
        );
      }, PAGE_CYCLE_MS);
    });
  }

  function dismiss(b) {
    const idx = bubbles.indexOf(b);
    if (idx < 0) return;
    clearPagingTimer(b);
    bubbles.splice(idx, 1);
    if (b.el.parentNode) b.el.parentNode.removeChild(b.el);
    renderThreadTags();
    notifyCount();
  }

  function createBubble(thread_id, turn_id) {
    const el = document.createElement('div');
    el.className = 'bubble';
    el.dataset.threadId = thread_id;
    el.dataset.turnId = turn_id;

    const threadEl = document.createElement('div');
    threadEl.className = 'bubble-thread';
    threadEl.hidden = true;

    const textEl = document.createElement('div');
    textEl.className = 'bubble-text';
    textEl.textContent = '...';

    el.appendChild(threadEl);
    el.appendChild(textEl);

    const entry = {
      id: bubbleId(thread_id, turn_id),
      thread_id,
      turn_id,
      text: '',
      status: 'open',
      label: null,
      el,
      textEl,
      threadEl,
    };

    el.addEventListener('click', () => dismiss(entry));

    return entry;
  }

  // Drop every bubble belonging to the given thread (any turn_id). Used when
  // a new turn starts on a thread that already had a closed/error bubble on
  // screen — we want the bubble to "update" rather than stack two of them.
  function evictThread(thread_id) {
    for (let i = bubbles.length - 1; i >= 0; i--) {
      const b = bubbles[i];
      if (b.thread_id === thread_id) {
        clearPagingTimer(b);
        bubbles.splice(i, 1);
        if (b.el.parentNode) b.el.parentNode.removeChild(b.el);
      }
    }
  }

  function pushBubble(entry) {
    // A real bubble arrived — drop the preview so it doesn't sit alongside.
    clearPreview();
    // Same thread = same bubble slot. Replace any prior bubble for this
    // thread (closed last_message, error, or in-flight stale turn) so the
    // pet only ever shows one bubble per thread.
    evictThread(entry.thread_id);
    bubbles.push(entry);
    root.appendChild(entry.el);
    while (bubbles.length > maxBubbles) {
      const evicted = bubbles.shift();
      clearPagingTimer(evicted);
      if (evicted.el.parentNode) evicted.el.parentNode.removeChild(evicted.el);
    }
    renderThreadTags();
    notifyCount();
  }

  function applyText(b) {
    b.textEl.textContent = stripInternalReplySuggestions(b.text) || '...';
    b.el.classList.toggle('error', b.status === 'error');
    b.el.classList.toggle('closed', b.status === 'closed');
    schedulePaging(b);
  }

  function open(thread_id, turn_id, initialText = '') {
    let b = find(thread_id, turn_id);
    if (!b) {
      b = createBubble(thread_id, turn_id);
      pushBubble(b);
    }
    b.text = initialText;
    b.status = 'open';
    applyText(b);
    return b;
  }

  // Update the entry's label from an incoming event, if it carried one.
  // Returns true when the visible tag would change (so callers can trigger
  // a re-render of the thread tags).
  function applyIncomingLabel(b, ev) {
    if (typeof ev?.thread_label !== 'string') return false;
    if (b.label === ev.thread_label) return false;
    b.label = ev.thread_label;
    return true;
  }

  function handle(ev) {
    if (!ev || typeof ev.type !== 'string') return;
    switch (ev.type) {
      case 'bubble.snapshot': {
        // A bubble that was already open before we connected. Treat it like
        // the open + (current text) state.
        open(ev.thread_id, ev.turn_id, ev.text ?? '');
        break;
      }
      case 'bubble.open': {
        open(ev.thread_id, ev.turn_id, '');
        break;
      }
      case 'bubble.delta': {
        let b = find(ev.thread_id, ev.turn_id);
        if (!b) {
          // Implicit open if delta arrives before we saw bubble.open
          // (the server may also do this; we mirror it here for safety).
          b = createBubble(ev.thread_id, ev.turn_id);
          pushBubble(b);
        }
        b.text += ev.text ?? '';
        b.status = 'open';
        applyText(b);
        break;
      }
      case 'bubble.close': {
        const b = find(ev.thread_id, ev.turn_id);
        if (!b) {
          // Closed without ever being open in our view — synthesize the final
          // bubble so the user still sees the message.
          const synth = createBubble(ev.thread_id, ev.turn_id);
          synth.text = ev.last_message ?? '';
          synth.status = 'closed';
          pushBubble(synth);
          applyText(synth);
        } else {
          b.text = ev.last_message ?? b.text ?? '';
          b.status = 'closed';
          applyText(b);
        }
        break;
      }
      case 'bubble.error': {
        let b = find(ev.thread_id, ev.turn_id);
        if (!b) {
          b = createBubble(ev.thread_id, ev.turn_id);
          pushBubble(b);
        }
        b.text = ev.message ?? 'error';
        b.status = 'error';
        applyText(b);
        break;
      }
      default:
        break;
    }
    // After dispatching, copy the (possibly fresh) thread_label onto every
    // bubble in this thread and re-render the tags. Channel renames + first
    // discovery both flow through here.
    if (typeof ev.thread_label === 'string') {
      let touched = false;
      for (const b of bubbles) {
        if (b.thread_id === ev.thread_id) {
          if (applyIncomingLabel(b, ev)) touched = true;
        }
      }
      if (touched) renderThreadTags();
    }
  }

  return {
    handle,
    showPreview,
    clearPreview,
    // Test seam.
    _state: () =>
      bubbles.map(({ thread_id, turn_id, text, status }) => ({
        thread_id,
        turn_id,
        text,
        status,
      })),
    _dismissAll: () => {
      while (bubbles.length) dismiss(bubbles[0]);
    },
  };
}

export function subscribeBubbles(serverUrl, ui, EventSourceCtor = globalThis.EventSource) {
  const url = `${serverUrl.replace(/\/$/, '')}/api/pet/bubbles`;
  const es = new EventSourceCtor(url);
  es.onmessage = (ev) => {
    try {
      const payload = JSON.parse(ev.data);
      if (payload?.type?.startsWith?.('bubble.')) ui.handle(payload);
    } catch (err) {
      console.warn('xangi-pets: bad bubble payload', ev.data, err);
    }
  };
  es.onerror = () => {
    console.debug('xangi-pets: bubble SSE disconnected, retrying');
  };
  return es;
}
