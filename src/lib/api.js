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

export function websocketUrl(params) {
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  const search = new URLSearchParams(params);
  return `${protocol}://${window.location.host}/ws?${search.toString()}`;
}
