#!/usr/bin/env node
// Uses only an isolated temporary database; never changes an installed backend.
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};
const binary = resolve(
  option(
    '--binary',
    join(
      root,
      'target',
      'dist',
      `log73-backend${process.platform === 'win32' ? '.exe' : ''}`,
    ),
  ),
);
const samples = Number(option('--samples', '24'));
const fixtureContacts = Number(option('--contacts', '1000'));
const logLevel = option('--log-level', 'info,log73_backend::auth=debug');
if (!Number.isInteger(samples) || samples < 8 || samples > 1000) {
  throw new Error('--samples must be an integer between 8 and 1000');
}
if (
  !Number.isInteger(fixtureContacts) ||
  fixtureContacts < 0 ||
  fixtureContacts > 100000
) {
  throw new Error('--contacts must be an integer between 0 and 100000');
}
const directory = await mkdtemp(join(tmpdir(), 'log73-benchmark-'));
const logPath = join(directory, 'backend.log');
const outputPath = option('--output', undefined);
const listener = createServer();
await new Promise((resolve, reject) => {
  listener.once('error', reject);
  listener.listen(0, '127.0.0.1', resolve);
});
const port = listener.address().port;
await new Promise((resolve) => listener.close(resolve));
const baseUrl = `http://127.0.0.1:${port}`;
const output = createWriteStream(logPath);
let child;
let spawnError;
let exited;
function startBackend() {
  spawnError = undefined;
  child = spawn(
    binary,
    [
      '--bind',
      `127.0.0.1:${port}`,
      '--config-dir',
      join(directory, 'config'),
      '--data-dir',
      join(directory, 'data'),
      '--app-dir',
      root,
      '--log-level',
      logLevel,
    ],
    { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] },
  );
  child.stdout.pipe(output, { end: false });
  child.stderr.pipe(output, { end: false });
  child.on('error', (error) => {
    spawnError = error;
  });
  exited = new Promise((resolve) => child.on('close', resolve));
}

async function stopBackend() {
  if (child && child.exitCode === null && !spawnError) {
    child.kill('SIGINT');
    const stopped = await Promise.race([
      exited.then(() => true),
      new Promise((resolve) => setTimeout(() => resolve(false), 5000)),
    ]);
    if (!stopped) child.kill('SIGKILL');
    await exited;
  }
}
const password = randomBytes(24).toString('hex');
const authorization = `Basic ${Buffer.from(`benchmark:${password}`).toString('base64')}`;
let login = false;
let completedRequests = 0;
const report = {
  platform: process.platform,
  binary,
  samples,
  fixture_contacts: Math.max(samples, fixtureContacts),
  log_level: logLevel,
  concurrency: 8,
  measurements: [],
};

async function request(
  path,
  { method = 'GET', body, authenticated = login } = {},
) {
  const start = performance.now();
  const response = await fetch(`${baseUrl}/api${path}`, {
    method,
    headers: {
      ...(authenticated ? { Authorization: authorization } : {}),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
    signal: AbortSignal.timeout(30000),
  });
  const text = await response.text();
  const elapsed = performance.now() - start;
  const payload =
    text && response.headers.get('content-type')?.includes('application/json')
      ? JSON.parse(text)
      : null;
  if (!response.ok)
    throw new Error(
      `${method} ${path}: HTTP ${response.status}: ${payload?.error ?? 'request failed'}`,
    );
  if (payload?.ok === false)
    throw new Error(`${method} ${path}: ${payload.error}`);
  completedRequests += 1;
  return { elapsed, payload };
}

async function measure(name, concurrency, path, makeOptions = () => ({})) {
  let next = 0;
  const durations = [];
  const start = performance.now();
  await Promise.all(
    Array.from({ length: concurrency }, async () => {
      while (next < samples) {
        const index = next++;
        const result = await request(path, makeOptions(index));
        durations.push(result.elapsed);
      }
    }),
  );
  durations.sort((left, right) => left - right);
  const percentile = (p) => durations[Math.ceil(durations.length * p) - 1];
  const result = {
    name,
    path,
    concurrency,
    requests: durations.length,
    p50_ms: percentile(0.5),
    p95_ms: percentile(0.95),
    max_ms: durations.at(-1),
    total_ms: performance.now() - start,
  };
  report.measurements.push(result);
  console.log(
    `${name}: p50=${result.p50_ms.toFixed(2)}ms p95=${result.p95_ms.toFixed(2)}ms max=${result.max_ms.toFixed(2)}ms`,
  );
}

async function waitReady() {
  const deadline = performance.now() + 30000;
  while (true) {
    if (spawnError) throw spawnError;
    if (child.exitCode !== null)
      throw new Error('Backend exited before becoming ready');
    try {
      await request('/modes');
      break;
    } catch {
      if (performance.now() >= deadline)
        throw new Error('Backend did not become ready in 30 seconds');
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
}

try {
  startBackend();
  await waitReady();
  for (const path of ['/modes', '/bands', '/logs']) {
    await measure(`login-disabled sequential ${path}`, 1, path);
    await measure(`login-disabled concurrent ${path}`, 8, path);
  }
  const contest = (await request('/contest-settings?contest_id=CQ-WW-CW'))
    .payload;
  const contestParams = { CQ_ZONE: '4' };
  for (const field of contest.setup_fields) {
    if (field.default !== null && field.default !== undefined)
      contestParams[field.key] = field.default;
  }
  const log = (
    await request('/logs', {
      method: 'POST',
      body: {
        name: 'Isolated benchmark',
        contest_id: 'CQ-WW-CW',
        station_callsign: 'N0CALL',
        contest_params: contestParams,
      },
    })
  ).payload;
  const contactsPath = `/logs/${log.id}/contacts`;
  const contact = (index) => ({
    method: 'POST',
    body: {
      meta: { clientId: `benchmark-${index}` },
      adif: {
        QSO_DATE_TIME_ON: Math.floor(Date.now() / 1000) + index,
        CALL: 'W1AW',
        STATION_CALLSIGN: 'N0CALL',
        CONTEST_ID: 'CQ-WW-CW',
        BAND: '20m',
        FREQ: 14025000,
        MODE: 'CW',
        RST_SENT: 599,
        RST_RCVD: 599,
        CQZ: 5,
        MY_CQ_ZONE: 4,
      },
    },
  });
  await measure('login-disabled QSO commits', 1, contactsPath, contact);
  // Seed realistic log sizes through bounded API batches, never by editing SQLite.
  for (let index = samples; index < fixtureContacts; index += 100) {
    const batch = Array.from(
      { length: Math.min(100, fixtureContacts - index) },
      (_, offset) => {
        const payload = contact(index + offset).body;
        payload.adif.CALL = `K${index + offset}ABC`;
        return payload;
      },
    );
    await request(contactsPath, { method: 'POST', body: batch });
  }
  // Reopen the private file-backed database to exercise a genuinely cold log cache.
  await stopBackend();
  startBackend();
  await waitReady();
  await measure('cold contact cache concurrent fill', 8, contactsPath);
  const cachedPaths = [
    '/config',
    '/radios',
    `/logs/${log.id}`,
    `/logs/${log.id}/qso-count`,
    `/logs/${log.id}/stats`,
    contactsPath,
    `${contactsPath}?callsign_prefix=W1`,
  ];
  for (const path of cachedPaths) {
    await measure(`warm DB cache sequential ${path}`, 1, path);
    await measure(`warm DB cache concurrent ${path}`, 8, path);
  }
  await measure('cached ADIF exports', 2, `/logs/${log.id}/adif`, () => ({
    method: 'POST',
  }));
  await measure(
    'cached Cabrillo exports',
    2,
    `/logs/${log.id}/cabrillo`,
    () => ({
      method: 'POST',
      body: { export_params: { LOCATION: 'OH', NAME: 'Benchmark' } },
    }),
  );
  const serialContest = (
    await request('/contest-settings?contest_id=RDXC%20(DX)')
  ).payload;
  const serialParams = Object.fromEntries(
    serialContest.setup_fields
      .filter((field) => field.default !== null && field.default !== undefined)
      .map((field) => [field.key, field.default]),
  );
  serialParams['CATEGORY-OPERATOR'] = 'MULTI-OP';
  serialParams['CATEGORY-TRANSMITTER'] = 'TWO';
  const serialLog = (
    await request('/logs', {
      method: 'POST',
      body: {
        name: 'Serial reservations',
        contest_id: 'RDXC (DX)',
        station_callsign: 'N0CALL',
        contest_params: serialParams,
      },
    })
  ).payload;
  const serialPath = `/logs/${serialLog.id}/serial-allocation`;
  await measure('cached serial state', 8, `${serialPath}?field_adif=STX`);
  await measure('durable serial reservations', 8, serialPath, () => ({
    method: 'POST',
    body: { field_adif: 'STX' },
  }));
  await Promise.all([
    measure(
      'cached counts during concurrent writes',
      8,
      `/logs/${log.id}/qso-count`,
    ),
    measure('concurrent QSO commits', 2, contactsPath, (index) =>
      contact(index + samples),
    ),
  ]);
  for (const hardware of [
    '/serial-ports',
    '/audio-devices/input',
    '/audio-devices/output',
  ]) {
    const [result] = await Promise.all([
      request(hardware),
      measure(`login-disabled modes during ${hardware}`, 8, '/modes'),
    ]);
    report.measurements.push({
      name: `enumeration ${hardware}`,
      elapsed_ms: result.elapsed,
    });
  }
  await request('/config', {
    method: 'PUT',
    body: {
      login_user: 'benchmark',
      login_password_change: password,
      login_password_confirm: password,
    },
  });
  login = true;
  report.cold_auth_ms = (await request('/modes')).elapsed;
  console.log(`cold authentication: ${report.cold_auth_ms.toFixed(2)}ms`);
  for (const path of ['/modes', '/bands', '/logs']) {
    await measure(`warm-auth sequential ${path}`, 1, path);
    await measure(`warm-auth concurrent ${path}`, 8, path);
  }
  await measure('warm-auth QSO commits', 1, contactsPath, (index) =>
    contact(index + 2 * samples),
  );
} finally {
  await stopBackend();
  await new Promise((resolve) => output.end(resolve));
  const logs = await readFile(logPath, 'utf8');
  report.password_verifications = (
    logs.match(/password verification completed/g) ?? []
  ).length;
  report.sqlite_settings = logs
    .split('\n')
    .find((line) => line.includes('configured SQLite connection'))
    ?.replace(/\u001b\[[0-9;]*m/g, '');
  report.db_commands_by_request = {};
  for (const rawLine of logs.split('\n')) {
    const line = rawLine.replace(/\u001b\[[0-9;]*m/g, '');
    if (!line.includes('request completed') || !line.includes('db_cache_hits='))
      continue;
    const method = line.match(/method=([A-Z]+)/)?.[1];
    const path = line.match(/path=([^\s}]+)/)?.[1];
    if (!method || !path) continue;
    const entry = (report.db_commands_by_request[`${method} ${path}`] ??= {
      requests: 0,
      read_commands: 0,
      write_commands: 0,
      cache_hits: 0,
      cache_misses: 0,
      cache_loads: 0,
    });
    entry.requests += 1;
    for (const [key, field] of Object.entries({
      read_commands: 'db_read_commands',
      write_commands: 'db_write_commands',
      cache_hits: 'db_cache_hits',
      cache_misses: 'db_cache_misses',
      cache_loads: 'db_cache_loads',
    })) {
      entry[key] += Number(line.match(new RegExp(`${field}=(\\d+)`))?.[1] ?? 0);
    }
  }
  report.cache_metrics_available =
    Object.keys(report.db_commands_by_request).length !== 0;
  report.completed_requests = completedRequests;
  if (outputPath) {
    await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`);
    await writeFile(`${outputPath}.log`, logs);
  }
  await rm(directory, { recursive: true, force: true });
}
console.log(JSON.stringify(report, null, 2));
if (args.includes('--verify-cache')) {
  if (!report.cache_metrics_available)
    throw new Error(
      'Backend did not emit cache counters; use the updated binary and info logging',
    );
  const loggedRequests = Object.values(report.db_commands_by_request).reduce(
    (sum, counters) => sum + counters.requests,
    0,
  );
  if (loggedRequests !== report.completed_requests) {
    throw new Error(
      `Incomplete cache request logs: ${loggedRequests} summaries for ${report.completed_requests} completed requests`,
    );
  }
  let readCommands = 0;
  let loads = 0;
  for (const [request, counters] of Object.entries(
    report.db_commands_by_request,
  )) {
    if (request.startsWith('GET ')) {
      if (counters.write_commands !== 0)
        throw new Error(`Unexpected write command for ${request}`);
      if (
        !request.endsWith('/contacts') &&
        (counters.read_commands !== 0 || counters.write_commands !== 0)
      ) {
        throw new Error(`Expected cache-only reads for ${request}`);
      }
      readCommands += counters.read_commands;
      loads += counters.cache_loads;
    }
    if (
      (request.endsWith('/adif') || request.endsWith('/cabrillo')) &&
      (counters.read_commands !== 0 || counters.write_commands !== 0)
    ) {
      throw new Error(`Expected cache-only export for ${request}`);
    }
  }
  if (readCommands !== 1 || loads !== 1)
    throw new Error(
      `Expected one coalesced cold contact load; saw ${readCommands} read commands and ${loads} loads`,
    );
  console.log(
    'Cache verification passed: one cold contact load; all other reads and exports bypassed SQLite',
  );
}
