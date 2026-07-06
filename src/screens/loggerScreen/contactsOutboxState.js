import {
  committedBackendContact,
  contactAdif,
  contactMeta,
  metaValue,
  sortContacts,
} from '../loggerScreenHelpers.js';

export function mergeCommittedPage(currentContacts, committedPage) {
  const committedById = new Map();
  const localUncommitted = [];

  for (const contact of currentContacts) {
    const id = metaValue(contact, 'id');
    if (metaValue(contact, 'status') === 'Committed' && id !== undefined) {
      committedById.set(String(id), contact);
    } else {
      localUncommitted.push(contact);
    }
  }

  for (const contact of committedPage) {
    const id = metaValue(contact, 'id');
    if (id === undefined || id === null) continue;
    const key = String(id);
    const existing = committedById.get(key);
    committedById.set(
      key,
      existing
        ? {
            meta: {
              ...contactMeta(existing),
              ...contactMeta(contact),
              status: 'Committed',
            },
            adif: {
              ...contactAdif(existing),
              ...contactAdif(contact),
            },
          }
        : {
            ...contact,
            meta: { ...contactMeta(contact), status: 'Committed' },
          },
    );
  }

  return sortContacts([...committedById.values(), ...localUncommitted]);
}

export function mergeResetCommittedPage(currentContacts, page) {
  const committedPage = page.map(committedBackendContact);
  const localUncommitted = currentContacts.filter(
    (contact) => metaValue(contact, 'status') !== 'Committed',
  );
  return sortContacts([...committedPage, ...localUncommitted]);
}

export function nextContactToCommit(allContacts, committingIds) {
  return allContacts.find((contact) => {
    const status = metaValue(contact, 'status');
    const clientId = metaValue(contact, 'clientId');
    const id = metaValue(contact, 'id');
    if (status === 'Pending') {
      return clientId && !committingIds.has(clientId);
    }
    if (status === 'Updating') {
      const updateKey = id ?? clientId;
      return updateKey && !committingIds.has(updateKey);
    }
    return false;
  });
}
