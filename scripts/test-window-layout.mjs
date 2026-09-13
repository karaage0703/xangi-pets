import assert from 'node:assert/strict';

import {
  centeredWindowPosition,
  fitWindowSize,
  isWindowPositionRecoverable,
} from '../src/lib/window-layout.js';

// The pet by itself keeps the scale-derived minimum window.
assert.deepEqual(
  fitWindowSize({ w: 280, h: 200 }, { width: 96, height: 104 }),
  { w: 280, h: 200 },
);

// A labelled bubble can exceed the old fixed 96px bubble allowance. Its
// measured height must grow the window, including room for the top shadow.
assert.deepEqual(
  fitWindowSize({ w: 280, h: 200 }, { width: 260, height: 202 }),
  { w: 280, h: 234 },
);

// Multiple bubbles grow the window from their complete rendered stack rather
// than relying on the fixed-height estimate.
assert.deepEqual(
  fitWindowSize({ w: 280, h: 200 }, { width: 260, height: 310 }),
  { w: 280, h: 342 },
);

// Fractional DOM measurements round outward so the final pixel is not clipped.
assert.deepEqual(
  fitWindowSize({ w: 280, h: 200 }, { width: 260.25, height: 202.25 }),
  { w: 280, h: 235 },
);

const monitors = [
  { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } },
  { position: { x: -1280, y: 0 }, size: { width: 1280, height: 1024 } },
];

assert.equal(
  isWindowPositionRecoverable({ x: 100, y: 100 }, { width: 280, height: 200 }, monitors),
  true,
);
assert.equal(
  isWindowPositionRecoverable({ x: -1250, y: 900 }, { width: 280, height: 200 }, monitors),
  true,
);
assert.equal(
  isWindowPositionRecoverable({ x: 2500, y: 100 }, { width: 280, height: 200 }, monitors),
  false,
);
assert.equal(
  isWindowPositionRecoverable({ x: 1900, y: 1060 }, { width: 280, height: 200 }, monitors),
  false,
);
assert.deepEqual(
  centeredWindowPosition(
    { width: 280, height: 200 },
    { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } },
  ),
  { x: 820, y: 440 },
);

console.log('window layout tests passed');
