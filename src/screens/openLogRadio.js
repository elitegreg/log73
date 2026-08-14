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
  return isClientRadio(radio) ? '[Remote Client]' : '';
}

export function visibleRadioOptions(radios) {
  return (Array.isArray(radios) ? radios : []).filter(
    (radio) => !isClientRadio(radio) || isClientRadioOnline(radio),
  );
}

export function preferredRadioSelection(currentId, radios) {
  const values = visibleRadioOptions(radios);
  if (values.some((radio) => String(radio?.id) === String(currentId))) {
    return String(currentId);
  }
  return String(values[0]?.id ?? '');
}
