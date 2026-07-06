export function shouldRequestSerialRefill({ current, remaining, threshold }) {
  return current === null || current === undefined || remaining <= threshold;
}

export function unavailableSerialMessage(current, remaining) {
  return current !== null && current !== undefined
    ? `Serial number refill failed; ${remaining} reserved serial numbers remain.`
    : 'No serial numbers are currently available. Retrying backend allocation.';
}
