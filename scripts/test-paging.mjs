// Unit test for the page-cycling logic added to bubble.js. Uses a fake DOM
// that simulates scrollHeight/clientHeight so we can exercise schedulePaging
// without a real browser.

import {
  bubblePageLayout,
  makeBubbleUI,
  stripInternalReplySuggestions,
} from '../src/lib/bubble.js';

class FakeElement {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.parentNode = null;
    this._classes = new Set();
    this._listeners = new Map();
    this._textContent = '';
    this.hidden = false;
    this.dataset = {};
    // Layout fakes — only meaningful on textEl. Default to "fits".
    this.scrollHeight = 0;
    this.clientHeight = 0;
    this.scrollTop = 0;
    this.classList = {
      add: (c) => this._classes.add(c),
      remove: (c) => this._classes.delete(c),
      toggle: (c, on) => {
        const want = on === undefined ? !this._classes.has(c) : !!on;
        if (want) this._classes.add(c); else this._classes.delete(c);
      },
      contains: (c) => this._classes.has(c),
    };
  }
  get className() { return [...this._classes].join(' '); }
  set className(v) { this._classes = new Set(String(v).split(/\s+/).filter(Boolean)); }
  set textContent(v) {
    this._textContent = String(v);
    // Fake layout: 14px whole-pixel lines and a viewport of exactly four
    // lines, matching the invariant maintained by main.js + styles.css.
    const lines = Math.max(1, Math.ceil(v.length / 20));
    this.scrollHeight = lines * 14;
    this.clientHeight = 56;
  }
  get textContent() { return this._textContent; }
  appendChild(child) {
    if (child.parentNode) child.parentNode.removeChild(child);
    child.parentNode = this;
    this.children.push(child);
    return child;
  }
  removeChild(child) {
    const i = this.children.indexOf(child);
    if (i >= 0) { this.children.splice(i, 1); child.parentNode = null; }
    return child;
  }
  addEventListener(type, fn) {
    if (!this._listeners.has(type)) this._listeners.set(type, []);
    this._listeners.get(type).push(fn);
  }
  dispatchEvent(type) {
    const ls = this._listeners.get(type) ?? [];
    for (const fn of ls) fn({ type });
  }
  hasClass(c) { return this._classes.has(c); }
}

globalThis.document = { createElement: (tag) => new FakeElement(tag) };

// Manual rAF/setInterval so we can step time deterministically.
let rafQueue = [];
globalThis.requestAnimationFrame = (fn) => { rafQueue.push(fn); return rafQueue.length; };
function flushRAF() {
  const q = rafQueue; rafQueue = [];
  for (const fn of q) fn();
}

let intervalId = 0;
const intervals = new Map();
globalThis.setInterval = (fn, ms) => {
  intervalId += 1;
  intervals.set(intervalId, { fn, ms });
  return intervalId;
};
globalThis.clearInterval = (id) => { intervals.delete(id); };
function tickInterval(id) {
  const e = intervals.get(id);
  if (!e) throw new Error(`interval ${id} missing`);
  e.fn();
}
function activeIntervals() { return intervals.size; }

function assert(cond, msg) {
  if (!cond) {
    console.error(`✗ ${msg}`);
    process.exit(1);
  }
  console.log(`✓ ${msg}`);
}

async function run() {
  console.log('--- paging unit tests ---');

  const expectedLayouts = [
    [1, 17, 68],
    [1.3, 22, 88],
    [1.6, 27, 108],
    [2, 34, 136],
    [2.5, 42, 168],
  ];
  for (const [scale, expectedLine, expectedPage] of expectedLayouts) {
    const layout = bubblePageLayout(scale);
    assert(layout.lineHeight === expectedLine, `${scale}x: whole-pixel line height`);
    assert(layout.pageHeight === expectedPage, `${scale}x: page is exactly four lines`);
  }

  const root = new FakeElement('div');
  const ui = makeBubbleUI({ root, maxBubbles: 2 });

  // 1. Short text: no overflow → no timer, scrollTop stays 0.
  ui.handle({ type: 'bubble.open', thread_id: 'A', turn_id: 'u1' });
  ui.handle({ type: 'bubble.delta', thread_id: 'A', turn_id: 'u1', text: 'short' });
  ui.handle({ type: 'bubble.close', thread_id: 'A', turn_id: 'u1', last_message: 'short' });
  flushRAF();
  assert(activeIntervals() === 0, 'short closed bubble: no paging timer');

  // 2. Streaming long text: bubble sticks to bottom (scrollTop = scrollHeight).
  ui.handle({ type: 'bubble.open', thread_id: 'B', turn_id: 'u2' });
  // Text long enough to overflow the four-line viewport.
  const longBase = 'x'.repeat(200);
  ui.handle({ type: 'bubble.delta', thread_id: 'B', turn_id: 'u2', text: longBase });
  flushRAF();
  const bState = ui._state().find((b) => b.thread_id === 'B');
  assert(bState && bState.text === longBase, 'streaming: text accumulated');
  // textEl is children[1] of the bubble div; bubble div is last in root for B.
  const bBubble = root.children[root.children.length - 1];
  const bTextEl = bBubble.children[1];
  assert(bTextEl.scrollHeight > bTextEl.clientHeight, 'streaming: textEl overflows');
  assert(bTextEl.scrollTop === bTextEl.scrollHeight, 'streaming: stuck to bottom');
  assert(activeIntervals() === 0, 'streaming: no paging timer while open');

  // 3. Close that long bubble → paging timer starts, scrollTop resets to 0.
  ui.handle({ type: 'bubble.close', thread_id: 'B', turn_id: 'u2', last_message: longBase });
  flushRAF();
  assert(activeIntervals() === 1, 'closed long: paging timer started');
  assert(bTextEl.scrollTop === 0, 'closed long: scrollTop reset to 0');

  // 4. Tick the interval → advance one whole page, visit the shorter final
  // page, then loop. All stops remain on line boundaries.
  const onlyId = [...intervals.keys()][0];
  const cy = bTextEl.clientHeight;
  tickInterval(onlyId);
  assert(bTextEl.scrollTop === cy, `tick 1: scrollTop=${cy}`);
  const maxScrollTop = bTextEl.scrollHeight - bTextEl.clientHeight;
  tickInterval(onlyId);
  assert(
    bTextEl.scrollTop === maxScrollTop,
    `tick 2: shorter final page shown at scrollTop=${maxScrollTop}`,
  );
  assert(bTextEl.scrollTop % 14 === 0, 'final page starts on a line boundary');
  tickInterval(onlyId);
  assert(bTextEl.scrollTop === 0, 'tick 3: loops to the first page');

  // 5. Dismiss the bubble → timer cleared.
  bBubble.dispatchEvent('click');
  assert(activeIntervals() === 0, 'dismiss: paging timer cleared');
  ui.handle({ type: 'bubble.delta', thread_id: 'B', turn_id: 'u2', text: 'late' });
  ui.handle({
    type: 'bubble.close',
    thread_id: 'B',
    turn_id: 'u2',
    last_message: `${longBase}late`,
  });
  assert(
    !ui._state().some((b) => b.thread_id === 'B' && b.turn_id === 'u2'),
    'dismiss: later events for the same turn do not recreate the bubble',
  );
  ui.handle({ type: 'bubble.open', thread_id: 'B', turn_id: 'u3' });
  assert(
    ui._state().some((b) => b.thread_id === 'B' && b.turn_id === 'u3'),
    'dismiss: a new turn on the same thread still appears',
  );
  ui._dismissAll();

  // 6. Replace via same-thread new turn → old timer cleared.
  ui.handle({ type: 'bubble.open', thread_id: 'C', turn_id: 'c1' });
  ui.handle({ type: 'bubble.delta', thread_id: 'C', turn_id: 'c1', text: longBase });
  ui.handle({ type: 'bubble.close', thread_id: 'C', turn_id: 'c1', last_message: longBase });
  flushRAF();
  assert(activeIntervals() === 1, 'C closed long: timer running');
  ui.handle({ type: 'bubble.open', thread_id: 'C', turn_id: 'c2' });
  flushRAF();
  // Old C bubble was evicted (and its timer with it). New C bubble is short → no timer.
  assert(activeIntervals() === 0, 'evictThread: prior thread timer cleared');

  // 7. Cap eviction (maxBubbles=2): 3rd thread evicts oldest, including timer.
  ui._dismissAll();
  ui.handle({ type: 'bubble.open', thread_id: 'X', turn_id: 'x' });
  ui.handle({ type: 'bubble.delta', thread_id: 'X', turn_id: 'x', text: longBase });
  ui.handle({ type: 'bubble.close', thread_id: 'X', turn_id: 'x', last_message: longBase });
  flushRAF();
  ui.handle({ type: 'bubble.open', thread_id: 'Y', turn_id: 'y' });
  ui.handle({ type: 'bubble.delta', thread_id: 'Y', turn_id: 'y', text: longBase });
  ui.handle({ type: 'bubble.close', thread_id: 'Y', turn_id: 'y', last_message: longBase });
  flushRAF();
  assert(activeIntervals() === 2, 'X+Y both have timers');
  ui.handle({ type: 'bubble.open', thread_id: 'Z', turn_id: 'z' });
  ui.handle({ type: 'bubble.delta', thread_id: 'Z', turn_id: 'z', text: 'tiny' });
  ui.handle({ type: 'bubble.close', thread_id: 'Z', turn_id: 'z', last_message: 'tiny' });
  flushRAF();
  // X was oldest → evicted by cap. Y still has timer. Z is short → no timer.
  assert(activeIntervals() === 1, 'cap evict: only Y timer remains (X cleared, Z none)');

  // 8. Internal reply-suggestion metadata never appears in the bubble, even
  // while the opening marker is still split across streaming chunks.
  ui._dismissAll();
  assert(
    stripInternalReplySuggestions(
      '回答本文\n<xangi_reply_suggestions>["続けて","詳しく"]</xangi_reply_suggestions>',
    ) === '回答本文',
    'reply suggestions: complete block hidden',
  );
  assert(
    stripInternalReplySuggestions('タグ <xangi_reply_suggestions> の説明') ===
      'タグ <xangi_reply_suggestions> の説明',
    'reply suggestions: inline explanation preserved',
  );
  ui.handle({ type: 'bubble.open', thread_id: 'R', turn_id: 'r1' });
  ui.handle({ type: 'bubble.delta', thread_id: 'R', turn_id: 'r1', text: '回答本文' });
  ui.handle({
    type: 'bubble.delta',
    thread_id: 'R',
    turn_id: 'r1',
    text: '\n<xangi_reply',
  });
  const replyBubble = root.children[root.children.length - 1];
  const replyTextEl = replyBubble.children[1];
  assert(
    replyTextEl.textContent === '回答本文',
    'reply suggestions: partial streaming marker hidden',
  );
  const finalWithSuggestions =
    '回答本文\n<xangi_reply_suggestions>["続けて","詳しく"]</xangi_reply_suggestions>';
  ui.handle({
    type: 'bubble.close',
    thread_id: 'R',
    turn_id: 'r1',
    last_message: finalWithSuggestions,
  });
  flushRAF();
  assert(
    replyTextEl.textContent === '回答本文',
    'reply suggestions: final block remains hidden',
  );

  // 9. Preview bubbles participate in the visible count and dismiss on click.
  ui._dismissAll();
  const counts = [];
  const previewRoot = new FakeElement('div');
  const previewUi = makeBubbleUI({
    root: previewRoot,
    onCountChange: (count) => counts.push(count),
  });
  previewUi.showPreview('preview');
  assert(counts.at(-1) === 1, 'preview: visible count enables click reception');
  previewRoot.children[0].dispatchEvent('click');
  assert(previewRoot.children.length === 0, 'preview: click removes the bubble');
  assert(counts.at(-1) === 0, 'preview: dismiss disables click reception');

  console.log('\nall paging tests passed.');
}

run().catch((err) => {
  console.error('FAILED:', err);
  process.exit(1);
});
