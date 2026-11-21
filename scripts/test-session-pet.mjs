import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createPetDisplayState, petPixels } from '../src/lib/pet-display.js';

// Run the actual window controller with a Tauri-style target-aware event bus.
const source = readFileSync(process.argv[2] || new URL('../src/session-pet.js', import.meta.url), 'utf8')
  .replace(/^import .*;\n/gm, '').replace('export async function', 'async function');
const factory = new Function('document', 'window', 'invoke', 'listen', 'getCurrentWindow',
  'FPS', 'loadImage', 'makeRenderer', 'createPetDisplayState', 'petPixels',
  'setInterval', 'clearInterval', 'URL', 'Blob', `${source}\nreturn startSessionPet;`);
const listeners = new Set();
function listen(event, handler, target) {
  const entry = { event, handler, target };
  listeners.add(entry);
  return Promise.resolve(() => listeners.delete(entry));
}
function emit(event, payload, target) {
  for (const entry of listeners) {
    if (entry.event === event && (!target || !entry.target || entry.target === target))
      entry.handler({ payload });
  }
}
function context(id, name, revision = 1) {
  return { revision, instanceName: 'test', agentName: name, spriteDirectory: id,
    petDisplay: { scale: 1, revision: 1 },
    session: { agentId: id, title: `${name}の会話`, summary: '実行中', state: 'thinking' } };
}
async function open(id, name, options = {}) {
  const calls = [];
  const nodes = [], events = new Map();
  const element = () => ({ textContent: '', classList: { add() {} },
    append() {}, before() {}, remove() {}, addEventListener() {} });
  const canvas = element();
  const document = { body: element(), getElementById: (id) => id === 'pet' ? canvas : element(),
    createElement: () => { const node = element(); nodes.push(node); return node; } };
  const window = { addEventListener: (event, fn) => events.set(event, fn) };
  const renderer = { setSize() {}, setState() {}, draw() { calls.push('draw'); }, tick() {}, getState: () => 'thinking' };
  const invoke = async (command) => {
    calls.push(command);
    if (command === 'get_session_pet_context') return context(id, name);
    if (command === 'get_pet_display') return { scale: 1, revision: 1 };
    return { image: [] };
  };
  const start = factory(document, window, invoke,
    (event, fn) => listen(event, fn),
    () => ({ listen: (event, fn) => listen(event, fn, id) }),
    10, options.loadImage || (async () => ({ width: 1536, height: 1872 })), () => renderer,
    createPetDisplayState, petPixels, () => 1, () => {}, URL, Blob);
  await start();
  return { nodes, document, canvas, calls, close: () => events.get('beforeunload')() };
}
const bolt = await open('bolt', 'bolt'), writer = await open('writer', '文香'), task = await open('task', 'タスク担当');
emit('pet://session-context', context('task', 'タスク担当', 20), 'task');
assert.equal(bolt.nodes[1].textContent, 'test · bolt');
assert.equal(writer.nodes[1].textContent, 'test · 文香');
assert.equal(task.nodes[1].textContent, 'test · タスク担当');
emit('pet://session-context', context('bolt', 'bolt', 21), 'bolt');
assert.equal(writer.nodes[2].textContent, '文香の会話');
assert.equal(task.nodes[2].textContent, 'タスク担当の会話');
emit('pet://session-context', context('bolt', '古い名前', 2), 'bolt');
assert.equal(bolt.nodes[1].textContent, 'test · bolt');
const resident = context('task', '常駐', 30);
resident.session.persistent = true;
emit('pet://session-context', resident, 'task');
assert.equal(task.nodes[3].textContent, '見守り中', 'resident status must not look like another running session');
assert.equal(bolt.nodes[3].textContent, '実行中');
resident.revision++;
resident.session.state = 'idle';
emit('pet://session-context', resident, 'task');
assert.equal(task.nodes[3].textContent, '待機中');
resident.revision++;
resident.session.summary = '接続中';
emit('pet://session-context', resident, 'task');
assert.equal(task.nodes[3].textContent, '接続中');
emit('pet://scale-changed', { scale: 2, revision: 2 });
for (const pet of [bolt, writer, task]) pet.close();
assert.equal(listeners.size, 0);
console.log('session pet window isolation tests passed');

let release;
const delayed = await open('delayed', '遅い画像', { loadImage: () => new Promise(resolve => { release = resolve; }) });
assert.ok(!delayed.calls.includes('session_pet_ready'), 'never show a label-only window');
assert.equal(delayed.canvas.height, 104, 'canvas must fit before an image loads');
release({width:1536,height:1872});
await new Promise(setImmediate);
assert.equal(delayed.calls.filter(c => c === 'session_pet_ready').length, 1);
assert.ok(delayed.calls.indexOf('draw') < delayed.calls.indexOf('session_pet_ready'));
delayed.close();
let fallbackRelease, attempts = 0;
const fallback = await open('fallback', '代替画像', { loadImage: async () => {
  if (attempts++ === 0) throw new Error('fixture image failure');
  return new Promise(resolve => { fallbackRelease = resolve; });
} });
await new Promise(setImmediate);
assert.ok(!fallback.calls.includes('session_pet_ready'));
fallbackRelease({width:1536,height:1872});
await new Promise(setImmediate);
assert.ok(fallback.calls.indexOf('draw') < fallback.calls.indexOf('session_pet_ready'));
fallback.close();
let cancelledRelease;
const cancelled = await open('cancelled', '終了済み', { loadImage: () => new Promise(resolve => { cancelledRelease = resolve; }) });
cancelled.close();
cancelledRelease({width:1536,height:1872});
await new Promise(setImmediate);
assert.ok(!cancelled.calls.includes('session_pet_ready'), 'completion during loading must not reveal a retired pet');
console.log('session pet first-frame and delayed/fallback loading tests passed');
