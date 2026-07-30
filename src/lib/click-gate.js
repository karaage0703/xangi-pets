// Keep the Tauri click-through flag aligned with every interactive overlay.
// Notifications are serialized so a quick false -> true transition cannot
// finish out of order and leave the window click-through by mistake.
export function makeClickGateController(notify) {
  if (typeof notify !== 'function') {
    throw new Error('makeClickGateController: notify is required');
  }

  let bubbleCount = 0;
  let modalDepth = 0;
  let lastQueuedActive = false;
  let queue = Promise.resolve();

  function sync() {
    const active = bubbleCount > 0 || modalDepth > 0;
    if (active === lastQueuedActive) return queue;
    lastQueuedActive = active;
    queue = queue.catch(() => {}).then(() => notify(active));
    return queue;
  }

  function setBubbleCount(count) {
    bubbleCount = Math.max(0, Number.isFinite(count) ? count : 0);
    return sync();
  }

  function pushModal() {
    modalDepth += 1;
    return sync();
  }

  function popModal() {
    modalDepth = Math.max(0, modalDepth - 1);
    return sync();
  }

  return {
    setBubbleCount,
    pushModal,
    popModal,
    _state: () => ({
      bubbleCount,
      modalDepth,
      active: bubbleCount > 0 || modalDepth > 0,
    }),
  };
}
