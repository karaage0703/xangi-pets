import assert from 'node:assert/strict';
import {
  findProfile,
  normalizeProfile,
  parseProfiles,
  upsertProfile,
} from '../src/lib/connection-profiles.js';

const normalized = normalizeProfile({ name: ' xangi-a ', url: 'http://localhost:18888/' });
assert.equal(normalized.name, 'xangi-a');
assert.equal(normalized.url, 'http://localhost:18888');
assert.equal(normalized.webUiEnabled, true);

assert.deepEqual(parseProfiles('not-json'), []);
assert.equal(parseProfiles(JSON.stringify([{ id: 'a', name: 'A', url: 'http://a' }])).length, 1);

let profiles = upsertProfile([], { id: 'xangi-a', name: 'xangi-a', url: 'http://a' });
profiles = upsertProfile(profiles, {
  id: 'xangi-a',
  name: 'xangi-a',
  url: 'http://b',
  webUiEnabled: false,
});
assert.equal(profiles.length, 1);
assert.equal(findProfile(profiles, 'xangi-a').url, 'http://b');
assert.equal(findProfile(profiles, 'xangi-a').webUiEnabled, false);

const added = normalizeProfile({ name: 'xangi-b', url: 'http://c' });
profiles = upsertProfile(profiles, added);
assert.equal(profiles.length, 2);
assert.notEqual(added.id, 'xangi-a');
assert.equal(findProfile(profiles, added.id).url, 'http://c');

console.log('connection profile tests passed');
