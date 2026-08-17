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
  const entry = messageEntryForConfig(config, mode, key);
  return entry ? actionFromTemplate(entry.target) : null;
}

export function messageEntryForConfig(config, mode, key) {
  const normalizedMode = normalizeMessageMode(mode);
  const normalizedKey = String(key ?? '')
    .trim()
    .toUpperCase();
  if (!normalizedKey) return null;
  return (
    parseMessageEntries(config).find(
      (candidate) =>
        candidate.mode === normalizedMode && candidate.key === normalizedKey,
    ) ?? null
  );
}

function messageFieldText(fields, key) {
  const value = fields?.[key];
  if (value === undefined || value === null) return '';
  return String(value).trim();
}

function cutNumberText(value) {
  return String(value ?? '')
    .trim()
    .toUpperCase()
    .replaceAll('9', 'N');
}

export function renderMessageTemplate(template, fields = {}) {
  return String(template ?? '')
    .replace(/\{([A-Z][A-Z0-9_]*)\}/g, (_match, key) => {
      if (key === 'RST_SENT' || key === 'SENTRSTCUT') {
        return cutNumberText(fields.RST_SENT);
      }
      return messageFieldText(fields, key);
    })
    .trim();
}

export function renderMessageForConfig(config, mode, key, fields = {}) {
  const entry = messageEntryForConfig(config, mode, key);
  if (!entry || actionFromTemplate(entry.target)) return null;
  return renderMessageTemplate(entry.target, fields);
}

export function messageSendBatches(messages, separateMessages = false) {
  if (messages.length === 0) return [];
  if (separateMessages) {
    return messages.map(({ key, text }) => ({ keys: [key], text }));
  }
  return [
    {
      keys: messages.map(({ key }) => key),
      text: messages.map(({ text }) => text).join(' '),
    },
  ];
}

export function completionTrackedTextRequest(requestId, text) {
  return {
    request_id: requestId,
    text,
    wait_for_completion: true,
  };
}
