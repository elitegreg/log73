// Recursively discover frontend tests and run them with node --test.
//
// Passes explicit file paths instead of shell globs so tests in nested
// directories are never silently skipped and behavior is identical across
// shells and Node versions.
import { spawnSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const SRC = join(fileURLToPath(new URL('..', import.meta.url)), 'src');
const EXCLUDED_DIRS = new Set(['node_modules', 'dist']);

function discoverTests(dir, found = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!EXCLUDED_DIRS.has(entry.name)) {
        discoverTests(join(dir, entry.name), found);
      }
    } else if (entry.isFile() && entry.name.endsWith('.test.js')) {
      found.push(join(dir, entry.name));
    }
  }
  return found;
}

const files = [...new Set(discoverTests(SRC))].sort();
if (files.length === 0) {
  console.error('run_frontend_tests: no tests found under src/');
  process.exit(1);
}
const result = spawnSync('node', ['--test', ...files], { stdio: 'inherit' });
process.exit(result.status ?? 1);
