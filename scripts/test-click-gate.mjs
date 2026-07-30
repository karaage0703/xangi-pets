import { makeClickGateController } from '../src/lib/click-gate.js';

function assert(cond, msg) {
  if (!cond) {
    console.error(`✗ ${msg}`);
    process.exit(1);
  }
  console.log(`✓ ${msg}`);
}

async function run() {
  console.log('--- click gate unit tests ---');

  const notifications = [];
  const gate = makeClickGateController(async (active) => {
    notifications.push(active);
  });

  await gate.setBubbleCount(1);
  assert(notifications.join(',') === 'true', 'bubble enables the click gate');

  await gate.pushModal();
  await gate.popModal();
  assert(
    notifications.join(',') === 'true',
    'closing a modal keeps the gate enabled while a bubble remains',
  );

  await gate.setBubbleCount(0);
  assert(
    notifications.join(',') === 'true,false',
    'removing the last bubble disables the click gate',
  );

  await gate.pushModal();
  await gate.setBubbleCount(1);
  await gate.setBubbleCount(0);
  assert(
    notifications.join(',') === 'true,false,true',
    'modal keeps the gate enabled while bubble count changes',
  );
  await gate.popModal();
  assert(
    notifications.join(',') === 'true,false,true,false',
    'closing the final modal disables an otherwise empty gate',
  );

  console.log('\nall click gate tests passed.');
}

run().catch((err) => {
  console.error('FAILED:', err);
  process.exit(1);
});
