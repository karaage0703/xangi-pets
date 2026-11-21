import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { FPS, loadImage, makeRenderer } from './lib/pet-renderer.js';
import { createPetDisplayState, petPixels } from './lib/pet-display.js';

// Display-only: these windows do not create sessions, send messages, write
// connection preferences, wander, or install a second notification stream.
export async function startSessionPet() {
  document.body.classList.add('session-pet');
  document.getElementById('bubbles').remove();
  const canvas = document.getElementById('pet');
  const label = document.createElement('div');
  label.className = 'session-pet-label';
  const name = document.createElement('strong');
  const title = document.createElement('span');
  const status = document.createElement('span');
  status.className = 'session-pet-status';
  label.append(name, title, status);
  canvas.before(label);
  let context = null;
  let renderer = null;
  let stopped = false;
  let imageGeneration = 0;
  let spriteAgent = null;
  let ready = false;
  const display = createPetDisplayState(null, (scale) => {
    const { w, h } = petPixels(scale);
    if (renderer) renderer.setSize(w, h);
    else { canvas.width = w; canvas.height = h; }
  });

  const initialSize = petPixels(display.scale);
  canvas.width = initialSize.w;
  canvas.height = initialSize.h;

  async function reveal() {
    if (ready || stopped) return;
    ready = true;
    document.body.classList.add('pet-ready');
    try { await invoke('session_pet_ready'); }
    catch (error) { ready = false; throw error; }
  }

  function makePet(image) {
    const { w, h } = petPixels(display.scale);
    return makeRenderer(canvas, image, w, h);
  }

  async function loadSprite(value) {
    const generation = ++imageGeneration;
    try {
      const { image: bytes } = await invoke('get_session_pet_asset');
      const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/webp' }));
      let image;
      try { image = await loadImage(url); } finally { URL.revokeObjectURL(url); }
      if (image.width !== 1536 || image.height !== 1872) throw new Error('スプライトの寸法が異なります');
      if (stopped || generation !== imageGeneration) return;
      renderer = makePet(image);
      renderer.setState(context.session.state);
      renderer.draw();
      await reveal();
    } catch (error) {
      if (stopped || generation !== imageGeneration) return;
      status.textContent = '標準画像で表示';
      label.title = `${value.session.title}\nAgent画像を取得できません: ${error}`;
      try {
        const image = await loadImage(`${value.serverUrl}/api/pet/asset/xangi/spritesheet.webp`);
        if (stopped || generation !== imageGeneration) return;
        renderer = makePet(image);
        renderer.setState(context.session.state);
        renderer.draw();
        await reveal();
      } catch (fallbackError) {
        status.textContent = '画像を取得できません';
        console.warn('xangi-pets: session sprite unavailable', fallbackError);
      }
    }
  }

  function update(value) {
    if (stopped || (context && value.revision < context.revision)) return;
    context = value;
    display.apply(value.petDisplay);
    name.textContent = value.instanceName ? `${value.instanceName} · ${value.agentName}` : value.agentName;
    title.textContent = value.session.title;
    status.textContent = value.session.persistent ? (value.session.summary === '接続中' ? '接続中' : value.session.state === 'idle' ? '待機中' : '見守り中') : (value.session.state === 'talking' ? '応答中' : '実行中');
    label.title = `${value.session.title}\n${value.session.summary}`;
    document.title = `${value.agentName} · ${value.session.title}`;
    if (renderer && renderer.getState() !== value.session.state) renderer.setState(value.session.state);
    const agent = `${value.session.agentId}:${value.spriteDirectory}`;
    if (spriteAgent !== agent) { spriteAgent = agent; void loadSprite(value); }
  }

  const unlistenScale = await listen('pet://scale-changed', ({ payload }) => display.apply(payload));
  const unlisten = await getCurrentWindow().listen('pet://session-context', ({ payload }) => update(payload));
  update(await invoke('get_session_pet_context'));
  display.apply(await invoke('get_pet_display'));
  const timer = setInterval(() => renderer?.tick(), 1000 / FPS);
  canvas.addEventListener('mousedown', (event) => {
    if (event.button === 0) void getCurrentWindow().startDragging();
  });
  window.addEventListener('keydown', async (event) => {
    if (event.key === 'p' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      try { display.apply(await invoke('cycle_pet_scale')); }
      catch (error) { status.textContent = 'サイズ変更に失敗しました'; console.warn(error); }
    }
  });
  window.addEventListener('beforeunload', () => {
    stopped = true;
    imageGeneration++;
    clearInterval(timer);
    unlisten();
    unlistenScale();
  }, { once: true });
}
