export async function apiJson(path, options = {}) {
  const response = await fetch(`/api${path}`, {
    ...options,
    headers: {
      ...(options.body ? { 'Content-Type': 'application/json' } : {}),
      ...(options.headers ?? {}),
    },
  });

  if (!response.ok) {
    throw new Error(`request failed: ${response.status}`);
  }

  return response.json();
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
    let message = `request failed: ${response.status}`;
    const contentType = response.headers.get('content-type') ?? '';
    try {
      if (contentType.includes('application/json')) {
        const payload = await response.json();
        message = payload?.error ?? message;
      } else {
        const text = await response.text();
        if (text.trim()) message = text.trim();
      }
    } catch {
      // Keep the fallback message if the error body cannot be parsed.
    }
    throw new Error(message);
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

export function websocketUrl(params) {
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  const search = new URLSearchParams(params);
  return `${protocol}://${window.location.host}/ws?${search.toString()}`;
}
