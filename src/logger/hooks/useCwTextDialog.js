import { useEffect, useRef, useState } from 'react';
import { createMessageRequestId, modeIsCw } from '../mainWindowHelpers';

export function useCwTextDialog({ radioMode, onSendCwText, callSignRef }) {
  const [isCwTextDialogOpen, setIsCwTextDialogOpen] = useState(false);
  const [cwTextCommittedWords, setCwTextCommittedWords] = useState([]);
  const [cwTextCurrentWord, setCwTextCurrentWord] = useState('');
  const cwTextInputRef = useRef(null);

  useEffect(() => {
    if (isCwTextDialogOpen) {
      cwTextInputRef.current?.focus();
    }
  }, [isCwTextDialogOpen]);

  useEffect(() => {
    if (!modeIsCw(radioMode) && isCwTextDialogOpen) {
      setIsCwTextDialogOpen(false);
      setCwTextCommittedWords([]);
      setCwTextCurrentWord('');
      callSignRef.current?.focus();
    }
  }, [callSignRef, isCwTextDialogOpen, radioMode]);

  function openCwTextDialog() {
    if (!modeIsCw(radioMode)) return;
    setCwTextCommittedWords([]);
    setCwTextCurrentWord('');
    setIsCwTextDialogOpen(true);
  }

  function closeCwTextDialog() {
    setIsCwTextDialogOpen(false);
    setCwTextCommittedWords([]);
    setCwTextCurrentWord('');
    callSignRef.current?.focus();
  }

  function sendCwTextWord(sendTrailingSpace) {
    const word = cwTextCurrentWord.trim().toUpperCase();
    if (!word) return;

    onSendCwText?.({
      request_id: createMessageRequestId(),
      text: sendTrailingSpace ? `${word} ` : word,
      wait_for_completion: false,
    });
    setCwTextCommittedWords((current) => [...current, word]);
    setCwTextCurrentWord('');
  }

  function handleCwTextInputChange(event) {
    setCwTextCurrentWord(String(event.target.value ?? '').replace(/\s+/g, ''));
  }

  function handleCwTextInputKeyDown(event) {
    if (event.key === ' ') {
      event.preventDefault();
      sendCwTextWord(true);
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      sendCwTextWord(false);
      closeCwTextDialog();
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      closeCwTextDialog();
      return;
    }

    if (event.key === 'Backspace' && cwTextCurrentWord.length === 0) {
      event.preventDefault();
    }
  }

  return {
    isCwTextDialogOpen,
    cwTextCommittedWords,
    cwTextCurrentWord,
    cwTextInputRef,
    openCwTextDialog,
    closeCwTextDialog,
    handleCwTextInputChange,
    handleCwTextInputKeyDown,
  };
}
