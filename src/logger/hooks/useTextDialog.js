import { useEffect, useRef, useState } from 'react';
import {
  createMessageRequestId,
  modeIsCw,
  modeIsDigital,
} from '../mainWindowHelpers.js';

export function textSendingIsEnabled(radioMode, digitalTextEnabled) {
  return (
    modeIsCw(radioMode) ||
    (modeIsDigital(radioMode) && Boolean(digitalTextEnabled))
  );
}

export function textWordForMode(value, radioMode) {
  const word = String(value ?? '').trim();
  return modeIsCw(radioMode) ? word.toUpperCase() : word;
}

export function useTextDialog({
  radioMode,
  digitalTextEnabled,
  onSendText,
  callSignRef,
}) {
  const [isTextDialogOpen, setIsTextDialogOpen] = useState(false);
  const [textCommittedWords, setTextCommittedWords] = useState([]);
  const [textCurrentWord, setTextCurrentWord] = useState('');
  const textInputRef = useRef(null);
  const textSendingEnabled = textSendingIsEnabled(
    radioMode,
    digitalTextEnabled,
  );

  useEffect(() => {
    if (isTextDialogOpen) {
      textInputRef.current?.focus();
    }
  }, [isTextDialogOpen]);

  useEffect(() => {
    if (!textSendingEnabled && isTextDialogOpen) {
      setIsTextDialogOpen(false);
      setTextCommittedWords([]);
      setTextCurrentWord('');
      callSignRef.current?.focus();
    }
  }, [callSignRef, isTextDialogOpen, textSendingEnabled]);

  function openTextDialog() {
    if (!textSendingEnabled) return;
    setTextCommittedWords([]);
    setTextCurrentWord('');
    setIsTextDialogOpen(true);
  }

  function closeTextDialog() {
    setIsTextDialogOpen(false);
    setTextCommittedWords([]);
    setTextCurrentWord('');
    callSignRef.current?.focus();
  }

  function sendTextWord(sendTrailingSpace) {
    const word = textWordForMode(textCurrentWord, radioMode);
    if (!word) return;

    onSendText?.({
      request_id: createMessageRequestId(),
      text: sendTrailingSpace ? `${word} ` : word,
      wait_for_completion: false,
    });
    setTextCommittedWords((current) => [...current, word]);
    setTextCurrentWord('');
  }

  function handleTextInputChange(event) {
    setTextCurrentWord(String(event.target.value ?? '').replace(/\s+/g, ''));
  }

  function handleTextInputKeyDown(event) {
    if (event.key === ' ') {
      event.preventDefault();
      sendTextWord(true);
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      sendTextWord(false);
      closeTextDialog();
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      closeTextDialog();
      return;
    }

    if (event.key === 'Backspace' && textCurrentWord.length === 0) {
      event.preventDefault();
    }
  }

  return {
    textSendingEnabled,
    isTextDialogOpen,
    textCommittedWords,
    textCurrentWord,
    textInputRef,
    openTextDialog,
    closeTextDialog,
    handleTextInputChange,
    handleTextInputKeyDown,
  };
}
