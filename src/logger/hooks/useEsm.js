import { useEffect, useState } from 'react';
import { ESM_ENABLED_STORAGE_KEY } from '../mainWindowHelpers';

export function useEsm() {
  const [esmEnabled, setEsmEnabled] = useState(() => {
    return localStorage.getItem(ESM_ENABLED_STORAGE_KEY) === '1';
  });
  const [esmRunCallsignAttempt, setEsmRunCallsignAttempt] = useState('');
  const [esmExchangeSentCallsign, setEsmExchangeSentCallsign] = useState('');

  useEffect(() => {
    localStorage.setItem(ESM_ENABLED_STORAGE_KEY, esmEnabled ? '1' : '0');
  }, [esmEnabled]);

  return {
    esmEnabled,
    setEsmEnabled,
    esmRunCallsignAttempt,
    setEsmRunCallsignAttempt,
    esmExchangeSentCallsign,
    setEsmExchangeSentCallsign,
  };
}
