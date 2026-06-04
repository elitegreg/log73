export const BAND_MAP_VFO_CALLSIGN = '*** VFO ***';
export const BAND_MAP_ROW_HEIGHT_PX = 22;

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

export function normalizeBandMapSpot(spot) {
  if (!spot || spot.id === undefined || spot.id === null) return null;
  const id = String(spot.id);
  const frequencyHz = normalizedFrequencyHz(
    spot.frequency_hz ?? spot.frequencyHz ?? spot.freq,
  );
  if (!frequencyHz) return null;

  return {
    ...spot,
    id,
    frequency_hz: frequencyHz,
    frequency_tenth_khz: frequencyTenthKhz(frequencyHz),
    call_de: String(spot.call_de ?? '')
      .trim()
      .toUpperCase(),
    call_dx: String(spot.call_dx ?? '')
      .trim()
      .toUpperCase(),
  };
}

function compareSpots(left, right) {
  const frequencyComparison =
    left.frequency_tenth_khz - right.frequency_tenth_khz;
  if (frequencyComparison !== 0) return frequencyComparison;

  const callsignComparison = left.call_dx.localeCompare(right.call_dx);
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
  });
}

export function addBandMapSpot(store, rawSpot) {
  const spot = normalizeBandMapSpot(rawSpot);
  if (!spot) return store ?? createBandMapSpotStore();

  const baseStore = store ?? createBandMapSpotStore();
  const spotsById = new Map(baseStore.spotsById);
  const sortedSpots = baseStore.sortedSpots.filter(
    (currentSpot) => currentSpot.id !== spot.id,
  );
  const insertIndex = sortedInsertIndex(sortedSpots, spot);
  sortedSpots.splice(insertIndex, 0, spot);
  spotsById.set(spot.id, spot);

  return { spotsById, sortedSpots };
}

export function removeBandMapSpot(store, id) {
  const baseStore = store ?? createBandMapSpotStore();
  const key = String(id);
  if (!baseStore.spotsById.has(key)) return baseStore;

  const spotsById = new Map(baseStore.spotsById);
  spotsById.delete(key);
  return {
    spotsById,
    sortedSpots: baseStore.sortedSpots.filter((spot) => spot.id !== key),
  };
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
      callsign: spot.call_dx,
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
