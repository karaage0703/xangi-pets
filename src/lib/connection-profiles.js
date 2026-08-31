export function normalizeProfile(input) {
  const name = String(input?.name || '').trim();
  const url = String(input?.url || '').trim().replace(/\/+$/, '');
  if (!name || !url) return null;
  return {
    id: String(input?.id || '').trim() || createProfileId(name),
    name,
    url,
    webUiEnabled: input?.webUiEnabled !== false,
  };
}

export function parseProfiles(raw) {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const seen = new Set();
    return parsed.flatMap((value) => {
      const profile = normalizeProfile(value);
      if (!profile || seen.has(profile.id)) return [];
      seen.add(profile.id);
      return [profile];
    });
  } catch {
    return [];
  }
}

export function upsertProfile(profiles, input) {
  const profile = normalizeProfile(input);
  if (!profile) return profiles;
  const index = profiles.findIndex((value) => value.id === profile.id);
  if (index < 0) return [...profiles, profile];
  return profiles.map((value, current) => (current === index ? profile : value));
}

export function findProfile(profiles, id) {
  return profiles.find((profile) => profile.id === id) || null;
}

function createProfileId(name) {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9\u3040-\u30ff\u3400-\u9fff]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 32);
  const suffix = Math.random().toString(36).slice(2, 8);
  return `${slug || 'xangi'}-${suffix}`;
}
