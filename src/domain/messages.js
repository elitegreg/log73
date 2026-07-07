import {
  normalizeMessageMode,
  parseMessageModeSectionHeader,
} from './messageModes.js';

export function parseMessageEntries(config) {
  const entries = [];
  let currentMode = null;

  for (const rawLine of String(config ?? '').split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const sectionMode = parseMessageModeSectionHeader(line);
    if (sectionMode) {
      currentMode = sectionMode;
      continue;
    }
    if (line.startsWith('#') || !currentMode) continue;
    const commaIndex = line.indexOf(',');
    if (commaIndex <= 0) continue;
    const keyAndLabel = line.slice(0, commaIndex).trim();
    const target = line.slice(commaIndex + 1).trim();
    const parts = keyAndLabel.split(/\s+/, 2);
    const key = String(parts[0] ?? '')
      .trim()
      .toUpperCase();
    if (!key.startsWith('F')) continue;
    entries.push({
      mode: currentMode,
      key,
      label: String(parts[1] ?? '').trim(),
      target,
    });
  }

  return entries;
}

export function actionFromTemplate(template) {
  const match = String(template ?? '')
    .trim()
    .match(/^\{\s*action\s*:\s*([^}]+?)\s*\}$/i);
  return match ? match[1].trim() : null;
}

export function messageActionForConfig(config, mode, key) {
  const normalizedMode = normalizeMessageMode(mode);
  const normalizedKey = String(key ?? '')
    .trim()
    .toUpperCase();
  if (!normalizedKey) return null;
  const entry = parseMessageEntries(config).find(
    (candidate) =>
      candidate.mode === normalizedMode && candidate.key === normalizedKey,
  );
  return entry ? actionFromTemplate(entry.target) : null;
}
