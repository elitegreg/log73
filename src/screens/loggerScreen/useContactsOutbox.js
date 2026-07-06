import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { callsignFilterPrefix } from '../../domain/dxcc.js';
import { apiJson } from '../../lib/api';
import {
  CONTACTS_PAGE_SIZE,
  CONTACT_COMMIT_RETRY_DELAY_MS,
  committedBackendContact,
  contactAdif,
  contactMeta,
  loadLocalContacts,
  markContactFailed,
  mergeContact,
  metaValue,
  saveLocalContacts,
  sortContacts,
} from '../loggerScreenHelpers.js';
import {
  mergeCommittedPage,
  mergeResetCommittedPage,
  nextContactToCommit,
} from './contactsOutboxState.js';

function callsignPrefixMatches(contact, callsignPrefix) {
  if (!callsignPrefix) return true;
  const callsign = String(contactAdif(contact)?.CALL ?? '')
    .trim()
    .toUpperCase();
  return callsign.startsWith(callsignPrefix);
}

export function useContactsOutbox({
  logId,
  numericLogId,
  sessionId,
  backendSocketStatus,
  notifyOperationalError,
  notifyOfflineCachingDegraded,
}) {
  const [allContacts, setAllContacts] = useState(() =>
    loadLocalContacts(logId),
  );
  const [debouncedCallsignSearch, setDebouncedCallsignSearch] = useState('');
  const [contactsLoadState, setContactsLoadState] = useState('initial-loading');
  const [hasMoreContacts, setHasMoreContacts] = useState(false);
  const [isLoadingMoreContacts, setIsLoadingMoreContacts] = useState(false);
  const committingContactIdsRef = useRef(new Set());
  const refreshContactsRef = useRef(() => Promise.resolve(false));
  const contactsLoadErrorNotifiedRef = useRef(false);
  const loadMoreContactsErrorNotifiedRef = useRef(false);
  const commitContactErrorNotifiedRef = useRef(false);
  const activeCallsignPrefixRef = useRef('');

  const handleDebouncedCallsignChange = useCallback((value) => {
    const normalizedValue = String(value ?? '')
      .trim()
      .toUpperCase();
    setDebouncedCallsignSearch(normalizedValue);
  }, []);

  useEffect(() => {
    if (!saveLocalContacts(logId, allContacts)) {
      notifyOfflineCachingDegraded();
    }
  }, [allContacts, logId, notifyOfflineCachingDegraded]);

  useEffect(() => {
    setDebouncedCallsignSearch('');
    activeCallsignPrefixRef.current = '';
  }, [numericLogId]);

  useEffect(() => {
    activeCallsignPrefixRef.current = callsignFilterPrefix(
      debouncedCallsignSearch,
    );
  }, [debouncedCallsignSearch]);

  const visibleContacts = useMemo(() => {
    const callsignPrefix = callsignFilterPrefix(debouncedCallsignSearch);
    if (!callsignPrefix) return allContacts;

    return allContacts.filter((contact) => {
      if (metaValue(contact, 'status') !== 'Committed') {
        return callsignPrefixMatches(contact, callsignPrefix);
      }
      return true;
    });
  }, [allContacts, debouncedCallsignSearch]);

  useEffect(() => {
    let isCancelled = false;
    let contactsLoadInFlightPromise = null;
    let offset = 0;
    let hasMore = true;
    const callsignPrefix = callsignFilterPrefix(debouncedCallsignSearch);
    activeCallsignPrefixRef.current = callsignPrefix;
    setHasMoreContacts(true);
    setIsLoadingMoreContacts(false);

    function contactsPagePath(pageOffset) {
      const params = new URLSearchParams({
        limit: String(CONTACTS_PAGE_SIZE),
        offset: String(pageOffset),
      });
      if (callsignPrefix) params.set('callsign_prefix', callsignPrefix);
      return `/logs/${numericLogId}/contacts?${params.toString()}`;
    }

    function loadContacts({ mode = 'refresh', reset = true } = {}) {
      if (contactsLoadInFlightPromise) return contactsLoadInFlightPromise;
      if (!reset && !hasMore) return Promise.resolve(false);

      if (mode === 'load-more') {
        setIsLoadingMoreContacts(true);
      } else {
        setContactsLoadState((currentState) => {
          if (mode === 'retry') return 'retrying';
          if (mode === 'initial') return 'initial-loading';
          if (currentState === 'initial-loading') return 'initial-loading';
          return 'refreshing';
        });
      }

      contactsLoadInFlightPromise = (async () => {
        try {
          if (reset) {
            offset = 0;
            hasMore = true;
            setHasMoreContacts(true);
          }

          if (isCancelled) return false;
          const page = await apiJson(contactsPagePath(offset));
          const committedPage = page.map(committedBackendContact);
          if (isCancelled) return false;

          if (reset) {
            setAllContacts((currentContacts) =>
              mergeResetCommittedPage(currentContacts, page),
            );
          } else if (committedPage.length > 0) {
            setAllContacts((currentContacts) =>
              mergeCommittedPage(currentContacts, committedPage),
            );
          }

          offset += committedPage.length;
          hasMore = committedPage.length === CONTACTS_PAGE_SIZE;
          setHasMoreContacts(hasMore);

          if (mode !== 'load-more') {
            contactsLoadErrorNotifiedRef.current = false;
            loadMoreContactsErrorNotifiedRef.current = false;
            setContactsLoadState('idle');
          } else {
            loadMoreContactsErrorNotifiedRef.current = false;
          }
          return committedPage.length > 0;
        } catch (error) {
          if (isCancelled) return false;
          if (mode === 'load-more') {
            if (!loadMoreContactsErrorNotifiedRef.current) {
              loadMoreContactsErrorNotifiedRef.current = true;
              notifyOperationalError(
                'loadMoreContacts',
                'Unable to load more contacts.',
                error,
                {
                  logId: numericLogId,
                  callsignPrefix,
                  offset,
                },
              );
            }
            return false;
          }
          if (!contactsLoadErrorNotifiedRef.current) {
            contactsLoadErrorNotifiedRef.current = true;
            notifyOperationalError(
              'loadContacts',
              'Unable to load backend contacts. Using local contacts until the backend reconnects.',
              error,
              {
                logId: numericLogId,
                callsignPrefix,
              },
            );
          }
          setContactsLoadState('idle');
          return false;
        } finally {
          if (mode === 'load-more') {
            setIsLoadingMoreContacts(false);
          }
          contactsLoadInFlightPromise = null;
        }
      })();

      return contactsLoadInFlightPromise;
    }

    refreshContactsRef.current = ({ mode = 'refresh', reset = true } = {}) =>
      loadContacts({ mode, reset });
    setContactsLoadState('initial-loading');
    loadContacts({ mode: 'initial', reset: true });
    return () => {
      isCancelled = true;
      refreshContactsRef.current = () => Promise.resolve(false);
    };
  }, [numericLogId, logId, debouncedCallsignSearch, notifyOperationalError]);

  useEffect(() => {
    const pendingContact = nextContactToCommit(
      allContacts,
      committingContactIdsRef.current,
    );
    if (!pendingContact) return;

    const commitKey =
      metaValue(pendingContact, 'status') === 'Pending'
        ? metaValue(pendingContact, 'clientId')
        : (metaValue(pendingContact, 'id') ??
          metaValue(pendingContact, 'clientId'));
    committingContactIdsRef.current.add(commitKey);

    async function commitContact(contact) {
      try {
        const responseBody = await apiJson(`/logs/${numericLogId}/contacts`, {
          method: 'POST',
          body: JSON.stringify({
            meta: { ...contactMeta(contact), logId: numericLogId },
            adif: { ...contactAdif(contact) },
          }),
        });
        if (responseBody.contact) {
          commitContactErrorNotifiedRef.current = false;
          const callsignPrefix = activeCallsignPrefixRef.current;
          if (
            !callsignPrefix ||
            callsignPrefixMatches(responseBody.contact, callsignPrefix)
          ) {
            setAllContacts((currentContacts) =>
              mergeContact(currentContacts, {
                ...responseBody.contact,
                meta: {
                  ...contactMeta(responseBody.contact),
                  clientId: metaValue(contact, 'clientId'),
                },
              }),
            );
          } else {
            setAllContacts((currentContacts) =>
              currentContacts.filter(
                (currentContact) =>
                  metaValue(currentContact, 'clientId') !==
                  metaValue(contact, 'clientId'),
              ),
            );
          }
        } else {
          notifyOperationalError(
            'commitContactMissing',
            'Contact upload failed: server response did not include a committed contact.',
            null,
            {
              logId: numericLogId,
              contactId:
                metaValue(contact, 'id') ??
                metaValue(contact, 'clientId') ??
                null,
            },
          );
          setAllContacts((currentContacts) =>
            markContactFailed(
              currentContacts,
              contact,
              'Contact upload failed: server response did not include a committed contact.',
            ),
          );
        }
      } catch (error) {
        if (!commitContactErrorNotifiedRef.current) {
          commitContactErrorNotifiedRef.current = true;
          notifyOperationalError(
            'commitContactRetry',
            'Unable to commit contact. Retrying.',
            error,
            {
              logId: numericLogId,
              contactId:
                metaValue(contact, 'id') ??
                metaValue(contact, 'clientId') ??
                null,
            },
          );
        }
        window.setTimeout(
          () =>
            setAllContacts((currentContacts) => sortContacts(currentContacts)),
          CONTACT_COMMIT_RETRY_DELAY_MS,
        );
      } finally {
        committingContactIdsRef.current.delete(commitKey);
      }
    }

    commitContact(pendingContact);
  }, [allContacts, numericLogId, notifyOperationalError]);

  const refreshContacts = useCallback(
    ({ mode = 'refresh', reset = true } = {}) =>
      refreshContactsRef.current({ mode, reset }),
    [],
  );

  const loadMoreContacts = useCallback(() => {
    if (contactsLoadState === 'initial-loading') return Promise.resolve(false);
    if (backendSocketStatus !== 'connected') return Promise.resolve(false);
    return refreshContactsRef.current({ mode: 'load-more', reset: false });
  }, [backendSocketStatus, contactsLoadState]);

  const upsertRemoteContact = useCallback(
    (contact) => {
      if (metaValue(contact, 'sessionId') === sessionId) return;
      const callsignPrefix = activeCallsignPrefixRef.current;
      if (!callsignPrefix || callsignPrefixMatches(contact, callsignPrefix)) {
        setAllContacts((currentContacts) =>
          mergeContact(currentContacts, contact),
        );
      }
    },
    [sessionId],
  );

  const removeRemoteContact = useCallback((id) => {
    setAllContacts((currentContacts) =>
      currentContacts.filter(
        (contact) => String(metaValue(contact, 'id')) !== String(id),
      ),
    );
  }, []);

  return {
    allContacts,
    setAllContacts,
    visibleContacts,
    debouncedCallsignSearch,
    handleDebouncedCallsignChange,
    contactsLoadState,
    hasMoreContacts,
    isLoadingMoreContacts,
    refreshContacts,
    loadMoreContacts,
    upsertRemoteContact,
    removeRemoteContact,
  };
}
