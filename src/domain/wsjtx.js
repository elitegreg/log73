import { parseAdifRecords } from './adifImport.js';
import { cabrilloTransmitterAdif } from './cabrilloTransmitter.js';
import { epochFromLegacyQsoDateTime } from './dateTime.js';

export const DEFAULT_WSJTX_PORT = 2237;
export const MAX_WSJTX_ADIF_LENGTH = 64 * 1024;

export function nextAvailableWsjtxPort(radios, start = DEFAULT_WSJTX_PORT) {
  const reserved = new Set(
    (Array.isArray(radios) ? radios : [])
      .filter((radio) => Boolean(radio?.wsjtx_enabled))
      .map((radio) => Number(radio?.wsjtx_port))
      .filter((port) => Number.isInteger(port)),
  );
  for (
    let port = Math.max(1024, Number(start) || DEFAULT_WSJTX_PORT);
    port <= 65535;
    port += 1
  ) {
    if (!reserved.has(port)) return port;
  }
  return DEFAULT_WSJTX_PORT;
}

export function wsjtxDataModeLocked(radio, mode, wsjtxTarget) {
  return (
    Boolean(wsjtxTarget) &&
    wsjtxTargetControlVisible(Boolean(radio?.wsjtx_enabled), mode)
  );
}

export function wsjtxTargetControlVisible(wsjtxEnabled, mode) {
  return (
    Boolean(wsjtxEnabled) &&
    String(mode ?? '')
      .trim()
      .toUpperCase() === 'DATA'
  );
}

export function wsjtxContactFromMessage({
  eventId,
  text,
  logId,
  sessionId,
  operatorCallsign,
  cabrilloTransmitterId,
}) {
  const normalizedEventId = String(eventId ?? '').trim();
  if (!normalizedEventId) throw new Error('WSJT-X event ID is required');
  if (
    typeof text !== 'string' ||
    new TextEncoder().encode(text).length > MAX_WSJTX_ADIF_LENGTH
  ) {
    throw new Error(
      `WSJT-X ADIF must be a string no larger than ${MAX_WSJTX_ADIF_LENGTH} bytes`,
    );
  }

  const records = parseAdifRecords(text);
  if (records.length !== 1) {
    throw new Error(
      `WSJT-X ADIF must contain exactly one QSO record; found ${records.length}`,
    );
  }

  const adif = { ...records[0] };
  const qsoDateTimeOn = epochFromLegacyQsoDateTime(adif);
  if (qsoDateTimeOn === null) {
    throw new Error('WSJT-X QSO_DATE/TIME_ON is missing or invalid');
  }
  delete adif.QSO_DATE;
  delete adif.TIME_ON;
  adif.QSO_DATE_TIME_ON = qsoDateTimeOn;

  for (const name of ['STATION_CALLSIGN', 'CALL', 'BAND', 'FREQ', 'MODE']) {
    if (String(adif[name] ?? '').trim() === '') {
      throw new Error(`WSJT-X ADIF field ${name} is required`);
    }
  }
  const frequency = Number.parseFloat(adif.FREQ);
  if (!Number.isFinite(frequency) || frequency <= 0) {
    throw new Error('WSJT-X ADIF field FREQ is invalid');
  }

  const normalizedOperator = String(operatorCallsign ?? '').trim();
  if (!Object.hasOwn(adif, 'OPERATOR') && normalizedOperator) {
    adif.OPERATOR = normalizedOperator;
  }
  if (!Object.hasOwn(adif, 'APP_LOG73_TX_ID')) {
    Object.assign(adif, cabrilloTransmitterAdif(cabrilloTransmitterId));
  }

  return {
    meta: {
      status: 'Pending',
      sessionId,
      logId,
      clientId: normalizedEventId,
      source: 'wsjtx',
      force: true,
    },
    adif,
  };
}
