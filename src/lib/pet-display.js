export const PET_SCALE_STEPS = [1, 1.5, 2, 2.5, 3, 4];

export function petPixels(scale) {
  return { w: Math.round(96 * scale), h: Math.round(104 * scale) };
}

export function createPetDisplayState(initial, onScale) {
  let scale = PET_SCALE_STEPS.includes(initial?.scale) ? initial.scale : 1;
  let revision = Number.isFinite(initial?.revision) ? initial.revision : -1;
  return {
    get scale() { return scale; },
    apply(value) {
      if (!PET_SCALE_STEPS.includes(value?.scale) || !Number.isFinite(value?.revision) || value.revision < revision) return false;
      revision = value.revision;
      if (value.scale === scale) return false;
      scale = value.scale;
      onScale(scale);
      return true;
    },
  };
}
