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
  const text = await response.text();
  return text.trim() ? JSON.parse(text) : null;
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

export async function bandMapSpots({ logId } = {}) {
  const query = new URLSearchParams();
  if (logId !== undefined && logId !== null) query.set('log_id', String(logId));
  const suffix = query.size > 0 ? `?${query.toString()}` : '';
  return apiJson(`/bandmap/spots${suffix}`);
}

export async function saveBandMapSpot(payload) {
  return apiJson('/bandmap/spots', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function deleteBandMapSpot(id) {
  return apiJson(`/bandmap/spots/${id}`, {
    method: 'DELETE',
  });
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

export function websocketUrl(endpoint, params = {}, baseUrl) {
  const resolvedBaseUrl = baseUrl ?? window.location.href;
  const url = new URL(endpoint, resolvedBaseUrl);
  if (url.protocol === 'http:') {
    url.protocol = 'ws:';
  } else if (url.protocol === 'https:') {
    url.protocol = 'wss:';
  } else if (url.protocol !== 'ws:' && url.protocol !== 'wss:') {
    throw new Error(`unsupported websocket URL protocol: ${url.protocol}`);
  }
  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined && value !== null) {
      url.searchParams.set(key, String(value));
    }
  });
  return url.toString();
}
