const DEFAULT_VERTICAL_PADDING = 32;
const DEFAULT_MINIMUM_VISIBLE_SIZE = 48;

function finiteDimension(value) {
  return Number.isFinite(value) && value >= 0 ? value : 0;
}

// Keep the scale-derived window size as a minimum, then grow its height to
// the rendered stage. The stage measurement includes every visible bubble
// and the pet, while the extra padding keeps shadows away from the top edge.
export function fitWindowSize(
  minimum,
  measured,
  {
    verticalPadding = DEFAULT_VERTICAL_PADDING,
  } = {},
) {
  const minW = finiteDimension(minimum?.w);
  const minH = finiteDimension(minimum?.h);
  const stageH = finiteDimension(measured?.height);
  return {
    // Width remains scale-derived. Measuring it here would feed the current
    // viewport width back through #bubbles { width: 100% } and could make the
    // transparent window grow on every ResizeObserver callback.
    w: Math.ceil(minW),
    h: Math.max(Math.ceil(minH), Math.ceil(stageH + verticalPadding)),
  };
}

function rectangle(value) {
  return {
    x: Number(value?.x),
    y: Number(value?.y),
    width: finiteDimension(value?.width),
    height: finiteDimension(value?.height),
  };
}

// A small but usable part of the window must remain on one monitor. This
// still lets users intentionally park a pet partly beyond a screen edge while
// detecting coordinates left behind by a disconnected or rearranged monitor.
export function isWindowPositionRecoverable(
  position,
  size,
  monitors,
  { minimumVisibleSize = DEFAULT_MINIMUM_VISIBLE_SIZE } = {},
) {
  const windowRect = rectangle({ ...position, ...size });
  if (
    !Number.isFinite(windowRect.x) ||
    !Number.isFinite(windowRect.y) ||
    windowRect.width <= 0 ||
    windowRect.height <= 0
  ) {
    return false;
  }

  const requiredWidth = Math.min(windowRect.width, finiteDimension(minimumVisibleSize));
  const requiredHeight = Math.min(windowRect.height, finiteDimension(minimumVisibleSize));
  return (monitors ?? []).some((monitor) => {
    const monitorRect = rectangle({ ...monitor?.position, ...monitor?.size });
    const visibleWidth = Math.max(
      0,
      Math.min(windowRect.x + windowRect.width, monitorRect.x + monitorRect.width) -
        Math.max(windowRect.x, monitorRect.x),
    );
    const visibleHeight = Math.max(
      0,
      Math.min(windowRect.y + windowRect.height, monitorRect.y + monitorRect.height) -
        Math.max(windowRect.y, monitorRect.y),
    );
    return visibleWidth >= requiredWidth && visibleHeight >= requiredHeight;
  });
}

export function centeredWindowPosition(size, monitor) {
  const windowSize = rectangle(size);
  const monitorRect = rectangle({ ...monitor?.position, ...monitor?.size });
  if (
    !Number.isFinite(monitorRect.x) ||
    !Number.isFinite(monitorRect.y) ||
    monitorRect.width <= 0 ||
    monitorRect.height <= 0
  ) {
    return null;
  }
  return {
    x: Math.round(monitorRect.x + (monitorRect.width - windowSize.width) / 2),
    y: Math.round(monitorRect.y + (monitorRect.height - windowSize.height) / 2),
  };
}
