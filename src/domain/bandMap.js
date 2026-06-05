export const BAND_MAP_VFO_CALLSIGN = '*** VFO ***';
export const BAND_MAP_CQ_CALLSIGN = '*** CQ ***';
export const BAND_MAP_IN_USE_CALLSIGN = '*** In Use ***';
export const BAND_MAP_ROW_HEIGHT_PX = 22;

export const BAND_MAP_SPOT_TYPES = {
  DX: 'dx',
  RBN: 'rbn',
  CQ: 'cq',
  IN_USE: 'in_use',
};

function normalizedFrequencyHz(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

export function frequencyTenthKhz(frequencyHz) {
  return Math.round(normalizedFrequencyHz(frequencyHz) / 100);
}

export function formatBandMapKhz(tenthKhz) {
  return (Math.round(Number(tenthKhz)) / 10).toFixed(1);
}

function normalizedSpotType(spot) {
  const type = String(spot?.spot_type ?? spot?.spotType ?? spot?.type ?? '')
    .trim()
    .toLowerCase();
  if (type === BAND_MAP_SPOT_TYPES.CQ) return BAND_MAP_SPOT_TYPES.CQ;
  if (type === BAND_MAP_SPOT_TYPES.IN_USE)
    return BAND_MAP_SPOT_TYPES.IN_USE;
  if (type === BAND_MAP_SPOT_TYPES.RBN) return BAND_MAP_SPOT_TYPES.RBN;
  return spot?.source === BAND_MAP_SPOT_TYPES.RBN
    ? BAND_MAP_SPOT_TYPES.RBN
    : BAND_MAP_SPOT_TYPES.DX;
}

function spotDisplayCallsign(spot) {
  switch (spot?.spot_type) {
    case BAND_MAP_SPOT_TYPES.CQ:
      return BAND_MAP_CQ_CALLSIGN;
    case BAND_MAP_SPOT_TYPES.IN_USE:
      return BAND_MAP_IN_USE_CALLSIGN;
    default:
      return spot?.call_dx ?? '';
  }
}

export function normalizeBandMapSpot(spot) {
  if (!spot || spot.id === undefined || spot.id === null) return null;
  const id = String(spot.id);
  const frequencyHz = normalizedFrequencyHz(
    spot.frequency_hz ?? spot.frequencyHz ?? spot.freq,
  );
  if (!frequencyHz) return null;

  const spotType = normalizedSpotType(spot);
  const callDx = String(spot.call_dx ?? spot.call ?? '')
    .trim()
    .toUpperCase();
  const normalizedSpot = {
    ...spot,
    id,
    spot_type: spotType,
    frequency_hz: frequencyHz,
    frequency_tenth_khz: frequencyTenthKhz(frequencyHz),
    call_de: String(spot.call_de ?? '')
      .trim()
      .toUpperCase(),
    call_dx: callDx,
  };

  return {
    ...normalizedSpot,
    display_callsign: spotDisplayCallsign(normalizedSpot),
  };
}

function compareSpots(left, right) {
  const frequencyComparison =
    left.frequency_tenth_khz - right.frequency_tenth_khz;
  if (frequencyComparison !== 0) return frequencyComparison;

  const callsignComparison = spotDisplayCallsign(left).localeCompare(
    spotDisplayCallsign(right),
  );
  if (callsignComparison !== 0) return callsignComparison;

  return left.id.localeCompare(right.id, undefined, { numeric: true });
}

function sortedInsertIndex(spots, spot) {
  let low = 0;
  let high = spots.length;

  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (compareSpots(spots[middle], spot) <= 0) low = middle + 1;
    else high = middle;
  }

  return low;
}

export function createBandMapSpotStore(spots = []) {
  return spots.reduce((store, spot) => addBandMapSpot(store, spot), {
    spotsById: new Map(),
    sortedSpots: [],
    cqFrequencyHzByBand: new Map(),
  });
}

export function addBandMapSpot(store, rawSpot) {
  const spot = normalizeBandMapSpot(rawSpot);
  if (!spot) return store ?? createBandMapSpotStore();

  const baseStore = store ?? createBandMapSpotStore();
  const spotsById = new Map(baseStore.spotsById);
  const cqFrequencyHzByBand = new Map(baseStore.cqFrequencyHzByBand ?? []);
  const previousSpot = spotsById.get(spot.id);
  if (previousSpot?.spot_type === BAND_MAP_SPOT_TYPES.CQ && previousSpot.band_meters) {
    cqFrequencyHzByBand.delete(String(previousSpot.band_meters));
  }
  const sortedSpots = baseStore.sortedSpots.filter(
    (currentSpot) => currentSpot.id !== spot.id,
  );
  const insertIndex = sortedInsertIndex(sortedSpots, spot);
  sortedSpots.splice(insertIndex, 0, spot);
  spotsById.set(spot.id, spot);
  if (spot.spot_type === BAND_MAP_SPOT_TYPES.CQ && spot.band_meters) {
    cqFrequencyHzByBand.set(String(spot.band_meters), spot.frequency_hz);
  }

  return { spotsById, sortedSpots, cqFrequencyHzByBand };
}

export function removeBandMapSpot(store, id) {
  const baseStore = store ?? createBandMapSpotStore();
  const key = String(id);
  if (!baseStore.spotsById.has(key)) return baseStore;

  const removedSpot = baseStore.spotsById.get(key);
  const spotsById = new Map(baseStore.spotsById);
  const cqFrequencyHzByBand = new Map(baseStore.cqFrequencyHzByBand ?? []);
  spotsById.delete(key);
  if (removedSpot?.spot_type === BAND_MAP_SPOT_TYPES.CQ && removedSpot.band_meters) {
    cqFrequencyHzByBand.delete(String(removedSpot.band_meters));
  }
  return {
    spotsById,
    sortedSpots: baseStore.sortedSpots.filter((spot) => spot.id !== key),
    cqFrequencyHzByBand,
  };
}

export function addCqBandMapSpot(store, frequencyHz, bandMeters) {
  const normalizedFrequency = normalizedFrequencyHz(frequencyHz);
  if (!normalizedFrequency || !bandMeters) return store ?? createBandMapSpotStore();
  return addBandMapSpot(store, {
    id: `cq:${bandMeters}`,
    spot_type: BAND_MAP_SPOT_TYPES.CQ,
    frequency_hz: normalizedFrequency,
    band_meters: bandMeters,
  });
}

export function addInUseBandMapSpot(store, frequencyHz) {
  const normalizedFrequency = normalizedFrequencyHz(frequencyHz);
  if (!normalizedFrequency) return store ?? createBandMapSpotStore();
  return addBandMapSpot(store, {
    id: `in-use:${frequencyTenthKhz(normalizedFrequency)}`,
    spot_type: BAND_MAP_SPOT_TYPES.IN_USE,
    frequency_hz: normalizedFrequency,
  });
}

export function lastCqFrequencyForBand(store, bandMeters) {
  if (!bandMeters) return null;
  return store?.cqFrequencyHzByBand?.get(String(bandMeters)) ?? null;
}

function isNavigableSpot(spot) {
  return (
    spot?.spot_type !== BAND_MAP_SPOT_TYPES.CQ &&
    spot?.spot_type !== BAND_MAP_SPOT_TYPES.IN_USE &&
    String(spot?.call_dx ?? '').trim() !== ''
  );
}

export function nextBandMapSpotAbove(store, vfoFrequencyHz) {
  const frequencyHz = normalizedFrequencyHz(vfoFrequencyHz);
  if (!frequencyHz) return null;
  return (
    (store?.sortedSpots ?? []).find(
      (spot) =>
        isNavigableSpot(spot) &&
        normalizedFrequencyHz(spot.frequency_hz) > frequencyHz,
    ) ?? null
  );
}

export function nextBandMapSpotBelow(store, vfoFrequencyHz) {
  const frequencyHz = normalizedFrequencyHz(vfoFrequencyHz);
  if (!frequencyHz) return null;
  const spots = store?.sortedSpots ?? [];
  for (let index = spots.length - 1; index >= 0; index -= 1) {
    const spot = spots[index];
    if (
      isNavigableSpot(spot) &&
      normalizedFrequencyHz(spot.frequency_hz) < frequencyHz
    )
      return spot;
  }
  return null;
}

export function bandMapRows(store, vfoFrequencyHz) {
  const spots = store?.sortedSpots ?? [];
  const vfoTenthKhz = frequencyTenthKhz(vfoFrequencyHz);
  const hasVfo = vfoTenthKhz > 0;
  const spotAtVfo = hasVfo
    ? spots.some((spot) => spot.frequency_tenth_khz === vfoTenthKhz)
    : false;
  const rows = [];
  let insertedVfoRow = false;

  for (const spot of spots) {
    if (
      hasVfo &&
      !spotAtVfo &&
      !insertedVfoRow &&
      spot.frequency_tenth_khz > vfoTenthKhz
    ) {
      rows.push(vfoRow(vfoTenthKhz));
      insertedVfoRow = true;
    }

    const isVfo = hasVfo && spot.frequency_tenth_khz === vfoTenthKhz;
    rows.push({
      key: `spot:${spot.id}`,
      type: 'spot',
      isVfo,
      marker: isVfo ? '➜' : '',
      frequencyTenthKhz: spot.frequency_tenth_khz,
      khz: formatBandMapKhz(spot.frequency_tenth_khz),
      callsign: spot.display_callsign ?? spotDisplayCallsign(spot),
      spot,
    });
  }

  if (hasVfo && !spotAtVfo && !insertedVfoRow) {
    rows.push(vfoRow(vfoTenthKhz));
  }

  return rows;
}

function vfoRow(vfoTenthKhz) {
  return {
    key: `vfo:${vfoTenthKhz}`,
    type: 'vfo',
    isVfo: true,
    marker: '➜',
    frequencyTenthKhz: vfoTenthKhz,
    khz: formatBandMapKhz(vfoTenthKhz),
    callsign: BAND_MAP_VFO_CALLSIGN,
  };
}
