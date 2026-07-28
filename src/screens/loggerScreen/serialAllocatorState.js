function positiveInteger(value) {
  const parsed = Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

export function normalizeSerialScope(value) {
  return String(value ?? '')
    .trim()
    .toLowerCase() === 'band'
    ? 'band'
    : 'global';
}

export function serialStateFromBackend(payload = {}) {
  const scope = normalizeSerialScope(payload.scope);
  const nextByBand = Object.fromEntries(
    Object.entries(payload.next_by_band ?? {})
      .map(([band, value]) => [band, positiveInteger(value)])
      .filter(([, value]) => value !== null),
  );
  return {
    fieldAdif: String(payload.field_adif ?? ''),
    scope,
    reservationRequired: payload.reservation_required === true,
    next: positiveInteger(payload.next),
    nextByBand,
  };
}

export function currentSerialForBand(state, band) {
  if (!state) return null;
  if (state.scope !== 'band') return positiveInteger(state.next);
  const bandKey = Object.keys(state.nextByBand ?? {}).find(
    (key) => key.toLowerCase() === String(band ?? '').toLowerCase(),
  );
  return positiveInteger(state.nextByBand?.[bandKey]);
}

export function mergeSerialStates(backendState, observedState) {
  if (
    !observedState ||
    backendState?.fieldAdif !== observedState.fieldAdif ||
    backendState?.scope !== observedState.scope
  ) {
    return backendState;
  }

  if (backendState.scope !== 'band') {
    return {
      ...backendState,
      next: Math.max(
        positiveInteger(backendState.next) ?? 1,
        positiveInteger(observedState.next) ?? 1,
      ),
    };
  }

  const nextByBand = { ...backendState.nextByBand };
  for (const [observedBand, observedNext] of Object.entries(
    observedState.nextByBand ?? {},
  )) {
    const backendBand =
      Object.keys(nextByBand).find(
        (band) => band.toLowerCase() === observedBand.toLowerCase(),
      ) ?? observedBand;
    nextByBand[backendBand] = Math.max(
      positiveInteger(nextByBand[backendBand]) ?? 1,
      positiveInteger(observedNext) ?? 1,
    );
  }
  return { ...backendState, nextByBand };
}

export function serialStateAfterContact(state, contact) {
  if (!state?.fieldAdif) return state;
  const adif = contact?.adif ?? {};
  const serial = positiveInteger(adif[state.fieldAdif]);
  if (serial === null) return state;

  if (state.scope === 'band') {
    const observedBand = String(adif.BAND ?? '').trim();
    if (!observedBand) return state;
    const band =
      Object.keys(state.nextByBand ?? {}).find(
        (key) => key.toLowerCase() === observedBand.toLowerCase(),
      ) ?? observedBand;
    const current = positiveInteger(state.nextByBand?.[band]) ?? 1;
    if (serial < current) return state;
    return {
      ...state,
      nextByBand: {
        ...state.nextByBand,
        [band]: serial + 1,
      },
    };
  }

  const current = positiveInteger(state.next) ?? 1;
  if (serial < current) return state;
  return {
    ...state,
    next: serial + 1,
  };
}

export function unavailableSerialMessage(scope, band) {
  return scope === 'band' && !band
    ? 'No serial number is available because the radio is outside a contest band.'
    : 'No serial number is currently available. Retrying the backend.';
}
