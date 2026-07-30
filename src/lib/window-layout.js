const DEFAULT_VERTICAL_PADDING = 32;

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
