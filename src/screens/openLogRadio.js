export function isClientRadio(radio) {
  return radio?.control_location === 'client';
}

export function isClientRadioOnline(radio) {
  return isClientRadio(radio) && radio?.client_online === true;
}

export function isOfflineClientRadio(radio) {
  return isClientRadio(radio) && radio?.client_online === false;
}

export function radioOwnershipLabel(radio) {
  if (!isClientRadio(radio)) return '[SERVER-SIDE]';
  return isClientRadioOnline(radio)
    ? '[CLIENT-SIDE · ONLINE]'
    : '[CLIENT-SIDE · OFFLINE]';
}

export function radioSelectionDetail(radio) {
  if (!radio) return 'Select a radio to view its control location.';
  if (!isClientRadio(radio)) {
    return 'Radio hardware is controlled by the Log73 backend.';
  }
  if (isClientRadioOnline(radio)) {
    return 'Radio hardware is controlled by Log73 Radio Client on the operator computer.';
  }
  return 'Start Log73 Radio Client on that computer before opening this radio.';
}

export function preferredRadioSelection(currentId, radios) {
  const values = Array.isArray(radios) ? radios : [];
  if (values.some((radio) => String(radio?.id) === String(currentId))) {
    return String(currentId);
  }
  const available = values.find((radio) => !isOfflineClientRadio(radio));
  return String((available ?? values[0])?.id ?? '');
}
