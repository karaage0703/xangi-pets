import assert from 'node:assert/strict';
import { createPetDisplayState, petPixels } from '../src/lib/pet-display.js';

const changes = [];
const first = createPetDisplayState({ scale: 1, revision: 0 }, (scale) => changes.push(['first', scale]));
const second = createPetDisplayState({ scale: 1, revision: 0 }, (scale) => changes.push(['second', scale]));
for (const pet of [first, second]) pet.apply({ scale: 2, revision: 2 });
assert.deepEqual(changes, [['first', 2], ['second', 2]]);
first.apply({ scale: 1.5, revision: 1 });
first.apply({ scale: 2, revision: 2 });
first.apply({ scale: Infinity, revision: 3 });
assert.equal(changes.length, 2, 'old or duplicate events must not rewind a pet');
const later = createPetDisplayState({ scale: first.scale, revision: 2 }, () => {});
assert.deepEqual(petPixels(later.scale), { w: 192, h: 208 }, 'newly opened pet uses the shared size');
assert.deepEqual(petPixels(4), { w: 384, h: 416 });
console.log('shared pet display tests passed');
