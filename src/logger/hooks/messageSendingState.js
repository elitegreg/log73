export function addActiveMessageRequest(activeRequests, requestId, keys) {
  return new Map(activeRequests).set(requestId, [...keys]);
}

export function removeActiveMessageRequest(activeRequests, requestId) {
  const nextRequests = new Map(activeRequests);
  nextRequests.delete(requestId);
  return nextRequests;
}

export function activeMessageKeysFromRequests(activeRequests) {
  const keys = new Set();
  for (const activeKeys of activeRequests.values()) {
    for (const key of activeKeys) keys.add(key);
  }
  return keys;
}
