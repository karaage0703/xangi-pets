export function normalizeProfile(input) {
  const name = String(input?.name || '').trim();
  const rawUrl = String(input?.url || '').trim().replace(/\/+$/, '');
  const url = instanceKey(rawUrl) || rawUrl;
  if (!name || !url) return null;
  return {
    id: String(input?.id || '').trim() || createProfileId(name),
    name,
    url,
    webUiEnabled: input?.webUiEnabled !== false,
    agentId: String(input?.agentId || '').trim(),
    spriteDirectory: String(input?.spriteDirectory || 'assets/xangi-pets').trim(),
    enabled: typeof input?.enabled === 'boolean' ? input.enabled : undefined,
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

export function instanceKey(raw) {
  try {
    const url = new URL(raw);
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) return '';
    url.search = ''; url.hash = '';
    return url.toString().replace(/\/+$/, '');
  } catch { return ''; }
}

export function instanceGroups(profiles) {
  const groups = new Map();
  for (const profile of profiles) {
    const key = instanceKey(profile.url);
    if (!key) continue;
    const group = groups.get(key) || { url: key, profile, enabled: false };
    if (profile.enabled === true && !group.enabled) { group.enabled = true; group.profile = profile; }
    groups.set(key, group);
  }
  return [...groups.values()];
}

export function setInstanceEnabled(profiles, profileId, enabled) {
  const selected = findProfile(profiles, profileId);
  if (!selected) return profiles;
  const key = instanceKey(selected.url);
  return profiles.map((profile) => instanceKey(profile.url) === key
    ? { ...profile, enabled: enabled && profile.id === selected.id }
    : profile);
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
