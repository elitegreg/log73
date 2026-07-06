export const RUN_MESSAGE_MODE = 'run';
export const SEARCH_AND_POUNCE_MESSAGE_MODE = 's&p';

export function normalizeMessageMode(mode) {
  switch (String(mode ?? '').trim().toLowerCase()) {
    case 'run':
      return RUN_MESSAGE_MODE;
    case 's&p':
    case 'sp':
    case 'search_and_pounce':
    case 'search and pounce':
      return SEARCH_AND_POUNCE_MESSAGE_MODE;
    default:
      return SEARCH_AND_POUNCE_MESSAGE_MODE;
  }
}

export function parseMessageModeSectionHeader(line) {
  const upper = String(line ?? '').trim().toUpperCase();
  if (upper.includes('RUN MESSAGES')) return RUN_MESSAGE_MODE;
  if (
    upper.includes('S&P MESSAGES') ||
    upper.includes('SP MESSAGES') ||
    upper.includes('SEARCH AND POUNCE MESSAGES')
  ) {
    return SEARCH_AND_POUNCE_MESSAGE_MODE;
  }
  return null;
}
