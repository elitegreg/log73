import { apiJson } from './api.js';

const MAX_TEXT_LENGTH = 2048;
const MAX_STACK_LENGTH = 8192;
const MAX_DETAILS_LENGTH = 8192;

function truncateText(value, maxLength = MAX_TEXT_LENGTH) {
  const text = String(value ?? '');
  return text.length <= maxLength ? text : text.slice(0, maxLength);
}

function serializableDetails(details) {
  if (details === undefined) return null;
  try {
    const text = JSON.stringify(details);
    if (!text) return null;
    if (text.length <= MAX_DETAILS_LENGTH) {
      return JSON.parse(text);
    }
    return {
      truncated: true,
      json: text.slice(0, MAX_DETAILS_LENGTH),
    };
  } catch {
    return { value: truncateText(details, MAX_DETAILS_LENGTH) };
  }
}

function serializeError(error) {
  if (!error) return null;
  if (error instanceof Error) {
    return {
      name: truncateText(error.name || 'Error'),
      message: truncateText(error.message),
      stack: error.stack ? truncateText(error.stack, MAX_STACK_LENGTH) : null,
    };
  }
  return {
    name: 'Error',
    message: truncateText(error),
    stack: null,
  };
}

export function errorMessage(error, fallback) {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error.trim()) return error;
  return fallback;
}

export async function reportClientError({
  source,
  message,
  error,
  details,
  url = window.location.href,
  userAgent = window.navigator.userAgent,
}) {
  const payload = {
    source: truncateText(source || 'frontend'),
    message: truncateText(message || ''),
    url: truncateText(url || ''),
    user_agent: truncateText(userAgent || ''),
    error: serializeError(error),
    details: serializableDetails(details),
  };

  try {
    const result = await apiJson('/client-errors', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
    return Boolean(result?.ok);
  } catch {
    return false;
  }
}

export function reportClientErrorLater(payload) {
  void reportClientError(payload);
}
