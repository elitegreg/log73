import assert from 'node:assert/strict';
import test from 'node:test';
import { createReconnectingSocket } from './reconnectingSocketController.js';

class FakeEvents {
  listeners = new Map();
  addEventListener(name, listener) {
    if (!this.listeners.has(name)) this.listeners.set(name, new Set());
    this.listeners.get(name).add(listener);
  }
  removeEventListener(name, listener) {
    this.listeners.get(name)?.delete(listener);
  }
  emit(name, event = {}) {
    for (const listener of [...(this.listeners.get(name) ?? [])])
      listener(event);
  }
  capture(name) {
    return [...(this.listeners.get(name) ?? [])];
  }
  listenerCount() {
    return [...this.listeners.values()].reduce(
      (total, listeners) => total + listeners.size,
      0,
    );
  }
}

class FakeSocket extends FakeEvents {
  readyState = 0;
  sent = [];
  closeCalls = 0;
  failSend = false;
  open() {
    this.readyState = 1;
    this.emit('open');
  }
  message(message) {
    this.emit('message', { data: JSON.stringify(message) });
  }
  send(text) {
    if (this.failSend) throw new Error('send failed');
    this.sent.push(JSON.parse(text));
  }
  close() {
    this.closeCalls += 1;
    this.readyState = 2; // Deliberately do not deliver close; recovery must not depend on it.
  }
}

const backendPolicy = {
  immediateTimeoutRecovery: true,
  foregroundConnectingGraceMs: 3000,
};

function fixture(options = {}) {
  let now = 0;
  let sequence = 0;
  let pingId = 0;
  let failuresRemaining = 0;
  const timers = new Map();
  const sockets = [];
  const calls = {
    statuses: [],
    opens: [],
    messages: [],
    closes: [],
    errors: [],
    messageErrors: [],
    debug: [],
  };
  let callbacks = {
    onStatusChange: (status) => calls.statuses.push(status),
    onOpen: (context) => calls.opens.push(context),
    onMessage: (message) => calls.messages.push(message),
    onClose: (details) => calls.closes.push(details),
    onError: (error, details) => calls.errors.push({ error, ...details }),
    onMessageError: (error, raw) => calls.messageErrors.push({ error, raw }),
    onDebug: (event, details) => calls.debug.push({ event, ...details }),
  };
  const win = new FakeEvents();
  const doc = new FakeEvents();
  doc.hidden = false;
  doc.visibilityState = 'visible';
  const environment = {
    window: win,
    document: doc,
    now: () => now,
    requestId: () => `ping-${++pingId}`,
    createSocket: (url) => {
      if (failuresRemaining > 0) {
        failuresRemaining -= 1;
        throw new Error('constructor failed');
      }
      const socket = new FakeSocket();
      socket.url = url;
      sockets.push(socket);
      return socket;
    },
    setTimeout: (callback, delay) => {
      const id = ++sequence;
      timers.set(id, { callback, due: now + delay });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  };
  const controller = createReconnectingSocket({
    url: 'ws://example.test/session-1',
    createPing: (requestId) => ({ type: 'ping', request_id: requestId }),
    isPong: (message) => message?.type === 'pong',
    getCallbacks: () => callbacks,
    environment,
    ...options,
  });
  return {
    controller,
    sockets,
    timers,
    calls,
    win,
    doc,
    environment,
    setCallbacks(next) {
      callbacks = { ...callbacks, ...next };
    },
    failConstructors(count) {
      failuresRemaining = count;
    },
    tick(duration) {
      const end = now + duration;
      let iterations = 0;
      while (true) {
        const next = [...timers]
          .filter(([, timer]) => timer.due <= end)
          .sort((a, b) => a[1].due - b[1].due)[0];
        if (!next) break;
        assert.ok(++iterations < 1000, 'timer loop must settle');
        const [id, timer] = next;
        timers.delete(id);
        now = timer.due;
        timer.callback();
      }
      now = end;
    },
    hide() {
      doc.hidden = true;
      doc.visibilityState = 'hidden';
      doc.emit('visibilitychange');
    },
    show() {
      doc.hidden = false;
      doc.visibilityState = 'visible';
      doc.emit('visibilitychange');
    },
  };
}

test('repeated error/close events create one reconnect with capped backoff, reset after open', () => {
  const f = fixture();
  f.controller.start();
  f.controller.start();
  assert.equal(f.sockets.length, 1);
  for (const delay of [2000, 4000, 8000, 16000, 16000]) {
    const socket = f.sockets.at(-1);
    const oldClose = socket.capture('close')[0];
    const oldError = socket.capture('error')[0];
    oldError({});
    oldError({});
    oldClose({ code: 1006 });
    assert.equal(socket.closeCalls, 1);
    assert.equal(f.timers.size, 1);
    const count = f.sockets.length;
    f.tick(delay - 1);
    assert.equal(f.sockets.length, count);
    f.tick(1);
    assert.equal(f.sockets.length, count + 1);
  }
  const socket = f.sockets.at(-1);
  socket.open();
  socket.emit('open');
  assert.equal(f.calls.opens.length, 1);
  socket.emit('close', { code: 1006 });
  const count = f.sockets.length;
  f.tick(2000);
  assert.equal(f.sockets.length, count + 1);
  f.controller.dispose();
});

test('constructor exceptions report an error and retry without duplicate listeners', () => {
  const f = fixture();
  f.failConstructors(2);
  f.controller.start();
  assert.equal(f.calls.errors[0].phase, 'connect');
  f.tick(2000);
  assert.equal(f.sockets.length, 0);
  f.tick(4000);
  assert.equal(f.sockets.length, 1);
  assert.equal(f.win.listenerCount(), 2);
  assert.equal(f.doc.listenerCount(), 1);
  f.controller.dispose();
});

for (const [name, policy] of [
  ['backend', backendPolicy],
  ['radio', {}],
]) {
  test(`${name} preserves connection and pong timeout recovery policy`, () => {
    for (const timeout of ['connect', 'pong', 'ping-send']) {
      const f = fixture(policy);
      f.controller.start();
      const socket = f.sockets[0];
      if (timeout !== 'connect') {
        socket.open();
        socket.failSend = timeout === 'ping-send';
        f.tick(15000);
        if (timeout === 'pong')
          assert.deepEqual(socket.sent, [
            { type: 'ping', request_id: 'ping-1' },
          ]);
      }
      if (timeout !== 'ping-send') f.tick(5000);
      assert.equal(socket.closeCalls, 1);
      assert.equal(f.sockets.length, name === 'backend' ? 2 : 1);
      if (name === 'radio') {
        f.tick(1999);
        assert.equal(f.sockets.length, 1);
        f.tick(1);
        assert.equal(f.sockets.length, 2);
      }
      f.controller.dispose();
    }
  });
}

test('any inbound traffic satisfies a pending ping, including malformed or nonmatching pong messages', () => {
  for (const message of [
    { type: 'pong', request_id: 'other' },
    { type: 'radio_state' },
    null,
    '{broken',
  ]) {
    const f = fixture(backendPolicy);
    f.controller.start();
    const socket = f.sockets[0];
    socket.open();
    f.tick(15000);
    if (typeof message === 'string') socket.emit('message', { data: message });
    else socket.message(message);
    f.tick(5000);
    assert.equal(f.sockets.length, 1);
    assert.equal(socket.closeCalls, 0);
    f.tick(10000);
    assert.equal(socket.sent.length, 2);
    f.controller.dispose();
  }
});

test('visibility pauses health deadlines, then foreground probes once despite repeated resume events', () => {
  const f = fixture(backendPolicy);
  f.controller.start();
  const socket = f.sockets[0];
  socket.open();
  f.tick(15000);
  f.hide();
  assert.equal(f.timers.size, 0);
  f.tick(60000);
  assert.equal(f.sockets.length, 1);
  f.show();
  f.win.emit('focus');
  f.win.emit('pageshow');
  assert.equal(socket.sent.length, 2);
  socket.message({ type: 'pong', request_id: 'ping-2' });
  f.tick(5000);
  assert.equal(socket.closeCalls, 0);
  f.controller.dispose();
});

test('foreground connecting grace applies only to backend; radio keeps its connection deadline', () => {
  for (const [policy, grace] of [
    [backendPolicy, true],
    [{}, false],
  ]) {
    const f = fixture(policy);
    f.controller.start();
    f.tick(1000);
    f.win.emit('focus');
    f.win.emit('pageshow');
    f.tick(1999);
    assert.equal(f.sockets.length, 1);
    f.tick(1);
    assert.equal(f.sockets.length, grace ? 2 : 1);
    if (!grace) {
      f.tick(2000);
      assert.equal(f.sockets[0].closeCalls, 1);
      f.tick(2000);
      assert.equal(f.sockets.length, 2);
    }
    f.controller.dispose();
  }
});

test('foreground bypasses a scheduled retry and handles closing or silently closed connections', () => {
  for (const state of [null, 2, 3]) {
    const f = fixture();
    f.controller.start();
    const socket = f.sockets[0];
    if (state === null) socket.emit('close', {});
    else socket.readyState = state;
    f.win.emit('focus');
    f.win.emit('pageshow');
    assert.equal(f.sockets.length, 2);
    f.sockets[1].open();
    f.tick(2000);
    assert.equal(f.sockets.length, 2);
    f.controller.dispose();
  }
});

test('disposal cancels every timer and listener, including already-queued stale callbacks', () => {
  for (const phase of [
    'connect',
    'foreground',
    'health',
    'pong',
    'reconnect',
  ]) {
    const f = fixture(backendPolicy);
    f.controller.start();
    const socket = f.sockets[0];
    const oldListeners = [...socket.listeners.values()].flatMap((listeners) => [
      ...listeners,
    ]);
    if (phase === 'foreground') f.win.emit('focus');
    if (phase === 'health' || phase === 'pong') socket.open();
    if (phase === 'pong') f.tick(15000);
    if (phase === 'reconnect') socket.emit('close', {});
    const oldTimers = [...f.timers.values()].map((timer) => timer.callback);
    f.controller.dispose();
    const recorded = Object.fromEntries(
      Object.entries(f.calls).map(([name, values]) => [name, [...values]]),
    );
    for (const callback of oldTimers) callback();
    for (const callback of oldListeners) callback({ data: '{broken' });
    f.win.emit('focus');
    f.show();
    f.tick(60000);
    f.controller.start();
    f.controller.dispose();
    assert.equal(f.sockets.length, 1);
    assert.equal(f.timers.size, 0);
    assert.equal(f.win.listenerCount(), 0);
    assert.equal(f.doc.listenerCount(), 0);
    assert.equal(socket.listenerCount(), 0);
    assert.deepEqual(f.calls, recorded);
  }
});

test('old connection callbacks and bound sends cannot affect a reconnect or a replacement URL/session', () => {
  const f = fixture(backendPolicy);
  f.controller.start();
  const old = f.sockets[0];
  const oldMessage = old.capture('message')[0];
  const oldOpen = old.capture('open')[0];
  const oldClose = old.capture('close')[0];
  old.open();
  const oldContext = f.calls.opens[0];
  f.tick(15000);
  const oldPongDeadline = [...f.timers.values()][0].callback;
  old.emit('close', {});
  f.tick(2000);
  const replacement = f.sockets[1];
  replacement.open();
  oldMessage({
    data: JSON.stringify({ type: 'log_entry', contact: 'old-log' }),
  });
  oldOpen();
  oldClose({});
  oldPongDeadline();
  assert.equal(f.calls.opens.length, 2);
  assert.equal(f.calls.messages.length, 0);
  assert.equal(oldContext.isCurrent(), false);
  assert.equal(oldContext.send({ type: 'set_wsjtx_target' }), false);
  assert.equal(replacement.sent.length, 0);
  f.controller.dispose();
  const next = createReconnectingSocket({
    url: 'ws://other.test/new-radio',
    environment: f.environment,
    createPing: (id) => ({ type: 'ping', request_id: id }),
    isPong: (message) => message?.type === 'pong',
  });
  next.start();
  f.sockets[2].open();
  oldMessage({ data: '{}' });
  oldPongDeadline();
  assert.equal(f.sockets[2].url, 'ws://other.test/new-radio');
  assert.equal(f.sockets.length, 3);
  assert.equal(f.sockets[2].closeCalls, 0);
  next.dispose();
});

test('send guards ignore non-open connections and callback changes do not reconnect', () => {
  const f = fixture();
  assert.equal(f.controller.send({}), false);
  f.controller.start();
  assert.equal(f.controller.send({}), false);
  f.sockets[0].open();
  const messages = [];
  f.setCallbacks({ onMessage: (message) => messages.push(message) });
  f.sockets[0].message({ type: 'radio_state', mode: 'CW' });
  assert.deepEqual(messages, [{ type: 'radio_state', mode: 'CW' }]);
  assert.equal(f.calls.messages.length, 0);
  assert.equal(f.sockets.length, 1);
  assert.equal(f.controller.send({ type: 'set_mode', mode: 'CW' }), true);
  f.sockets[0].readyState = 2;
  assert.equal(f.controller.send({}), false);
  f.controller.dispose();
  assert.equal(f.controller.send({}), false);
});

test('each successful reconnect runs protocol recovery once with a generation-bound send', () => {
  const f = fixture();
  let refreshes = 0;
  f.setCallbacks({
    onOpen: ({ send }) => {
      refreshes += 1;
      send({ type: 'set_bandmap_enabled', enabled: true });
      send({ type: 'set_wsjtx_target', enabled: true });
      send({ type: 'set_digital_io_target', enabled: true });
    },
  });
  f.controller.start();
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const socket = f.sockets.at(-1);
    socket.open();
    socket.emit('open');
    assert.equal(socket.sent.length, 3);
    if (attempt === 0) {
      socket.emit('close', {});
      f.tick(2000);
    }
  }
  assert.equal(refreshes, 2);
  f.controller.dispose();
});

test('protocol exceptions are reported without treating valid transport traffic as a disconnect', () => {
  const f = fixture(backendPolicy);
  f.setCallbacks({
    onMessage() {
      throw new Error('contact handler failed');
    },
  });
  f.controller.start();
  f.sockets[0].open();
  f.tick(15000);
  const message = { type: 'wsjtx_logged_adif', event_id: 'event-1', log_id: 7 };
  f.sockets[0].message(message);
  assert.equal(
    f.calls.messageErrors[0].error.message,
    'contact handler failed',
  );
  assert.deepEqual(JSON.parse(f.calls.messageErrors[0].raw), message);
  f.tick(5000);
  assert.equal(f.sockets.length, 1);
  f.controller.dispose();
});

test('delayed open callbacks cannot send into a replacement connection or report stale failures', async () => {
  const f = fixture();
  let finishOpen;
  let failOpen;
  let lateSend;
  let oldIsCurrent;
  f.setCallbacks({
    onOpen: ({ send, isCurrent }) => {
      oldIsCurrent = isCurrent;
      const work = new Promise((resolve) => {
        finishOpen = resolve;
      }).then(() => {
        lateSend = send({ type: 'set_bandmap_enabled', enabled: true });
      });
      const failure = new Promise((resolve, reject) => {
        failOpen = reject;
      });
      return Promise.all([work, failure]);
    },
  });
  f.controller.start();
  f.sockets[0].open();
  f.sockets[0].emit('close', {});
  f.setCallbacks({ onOpen: (context) => f.calls.opens.push(context) });
  f.tick(2000);
  f.sockets[1].open();
  finishOpen();
  failOpen(new Error('obsolete recovery failure'));
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(oldIsCurrent(), false);
  assert.equal(lateSend, false);
  assert.equal(f.sockets[1].sent.length, 0);
  assert.equal(f.calls.errors.length, 0);
  f.controller.dispose();
});
