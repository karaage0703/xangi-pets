export const CELL_W = 192;
export const CELL_H = 208;
export const COLS = 8;
export const ROWS = 9;

// Animation row layout: hatch-pet `references/animation-rows.md` convention.
export const ROW_NAMES = [
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

export const FPS = 6;

export function loadImage(src) {
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

export function makeRenderer(canvas, image, drawW, drawH) {
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
