import assert from 'node:assert/strict';
import {
  findProfile,
  normalizeProfile,
  parseProfiles,
  upsertProfile,
  instanceGroups,
  setInstanceEnabled,
} from '../src/lib/connection-profiles.js';

const normalized = normalizeProfile({ name: ' xangi-a ', url: 'http://localhost:18888/' });
assert.equal(normalized.name, 'xangi-a');
assert.equal(normalized.url, 'http://localhost:18888');
assert.equal(normalized.webUiEnabled, true);
assert.equal(normalized.agentId, '');
assert.equal(normalized.spriteDirectory, 'assets/xangi-pets');
assert.equal(normalized.enabled, undefined);

const multiple = parseProfiles(JSON.stringify([
  {id:'a',name:'A',url:'http://a/',enabled:true},
  {id:'a-other-agent',name:'A other',url:'http://a',enabled:false},
  {id:'b',name:'B',url:'http://b',enabled:true},
]));
assert.equal(instanceGroups(multiple).length,2);
const aOff=setInstanceEnabled(multiple,'a',false);
assert.equal(instanceGroups(aOff)[0].enabled,false);
assert.equal(instanceGroups(aOff)[1].enabled,true,'other instance remains ON');
const aOther=setInstanceEnabled(aOff,'a-other-agent',true);
assert.equal(aOther.find(p=>p.id==='a').enabled,false);
assert.equal(aOther.find(p=>p.id==='a-other-agent').enabled,true);
assert.deepEqual(parseProfiles(JSON.stringify(aOther)),aOther,'ON/OFF survives restart');

const sameInstance = parseProfiles(JSON.stringify([
  { id: 'agent-a', name: 'A', url: 'http://localhost:18888', agentId: ' a ' },
  { id: 'agent-b', name: 'B', url: 'http://localhost:18888', agentId: 'b', spriteDirectory: 'assets/pets/b' },
]));
assert.equal(sameInstance.length, 2);
assert.equal(sameInstance[0].agentId, 'a');
assert.equal(sameInstance[1].spriteDirectory, 'assets/pets/b');
assert.deepEqual(parseProfiles(JSON.stringify(sameInstance)), sameInstance);

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
