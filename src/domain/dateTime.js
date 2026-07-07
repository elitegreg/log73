export function epochFromLegacyQsoDateTime(entry) {
  const adif = entry?.adif ?? entry ?? {};
  const date = String(adif.QSO_DATE ?? '');
  const time = String(adif.TIME_ON ?? '');

  if (!/^\d{8}$/.test(date) || !/^\d{6}$/.test(time)) {
    return null;
  }

  return Math.floor(
    Date.UTC(
      Number.parseInt(date.slice(0, 4), 10),
      Number.parseInt(date.slice(4, 6), 10) - 1,
      Number.parseInt(date.slice(6, 8), 10),
      Number.parseInt(time.slice(0, 2), 10),
      Number.parseInt(time.slice(2, 4), 10),
      Number.parseInt(time.slice(4, 6), 10),
    ) / 1000,
  );
}

export function formatUtcDateTime(epoch) {
  if (!Number.isFinite(epoch)) return '';
  const date = new Date(epoch * 1000);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, '0');
  const day = String(date.getUTCDate()).padStart(2, '0');
  const hour = String(date.getUTCHours()).padStart(2, '0');
  const minute = String(date.getUTCMinutes()).padStart(2, '0');
  const second = String(date.getUTCSeconds()).padStart(2, '0');
  return `${year}-${month}-${day} ${hour}:${minute}:${second}`;
}

export function parseUtcDateTime(value) {
  const match = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/.exec(
    String(value ?? '').trim(),
  );
  if (!match) return null;

  const [, yearText, monthText, dayText, hourText, minuteText, secondText] =
    match;
  const year = Number.parseInt(yearText, 10);
  const month = Number.parseInt(monthText, 10);
  const day = Number.parseInt(dayText, 10);
  const hour = Number.parseInt(hourText, 10);
  const minute = Number.parseInt(minuteText, 10);
  const second = Number.parseInt(secondText, 10);
  const milliseconds = Date.UTC(year, month - 1, day, hour, minute, second);
  const date = new Date(milliseconds);

  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day ||
    date.getUTCHours() !== hour ||
    date.getUTCMinutes() !== minute ||
    date.getUTCSeconds() !== second
  ) {
    return null;
  }

  return Math.floor(milliseconds / 1000);
}
