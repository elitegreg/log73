function unwrapLegacySuccessPayload(payload) {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
    return payload;
  }
  if (payload.ok === false) {
    const error = new Error(payload.error ?? 'request failed');
    error.payload = payload;
    throw error;
  }
  if (payload.ok !== true) {
    return payload;
  }

  const entries = Object.entries(payload).filter(([key]) => key !== 'ok');
  if (entries.length === 1) {
    return entries[0][1];
  }
  return Object.fromEntries(entries);
}

async function errorFromResponse(response) {
  let message = `request failed: ${response.status}`;
  const contentType = response.headers.get('content-type') ?? '';

  try {
    if (contentType.includes('application/json')) {
      const payload = await response.json();
      message = payload?.error ?? message;
      const error = new Error(message);
      error.payload = payload;
      return error;
    } else {
      const text = await response.text();
      if (text.trim()) message = text.trim();
    }
  } catch {
    // Keep the fallback message if the error body cannot be parsed.
  }

  return new Error(message);
}

export async function apiJson(path, options = {}) {
  const response = await fetch(`/api${path}`, {
    ...options,
    headers: {
      ...(options.body ? { 'Content-Type': 'application/json' } : {}),
      ...(options.headers ?? {}),
    },
  });

  if (!response.ok) {
    throw await errorFromResponse(response);
  }

  if (response.status === 204) return null;
  return unwrapLegacySuccessPayload(await response.json());
}

export async function apiDownload(path, options = {}) {
  const response = await fetch(`/api${path}`, {
    ...options,
    headers: {
      ...(options.body ? { 'Content-Type': 'application/json' } : {}),
      ...(options.headers ?? {}),
    },
  });

  if (!response.ok) {
    throw await errorFromResponse(response);
  }

  const blob = await response.blob();
  const contentDisposition = response.headers.get('content-disposition') ?? '';
  const filenameMatch = contentDisposition.match(/filename="([^"]+)"/i);
  return {
    blob,
    filename: filenameMatch?.[1] ?? 'download.log',
  };
}

export async function supercheckpartial() {
  return apiJson('/supercheckpartial');
}

export async function dxcc() {
  return apiJson('/dxcc');
}

export async function dxclusterSpots() {
  return apiJson('/dxcluster/spots');
}

export async function saveDxclusterSpot(payload) {
  return apiJson('/dxcluster/spots', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function websocketUrl(params) {
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  const search = new URLSearchParams(params);
  return `${protocol}://${window.location.host}/ws?${search.toString()}`;
}
