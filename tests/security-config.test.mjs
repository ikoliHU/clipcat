import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'node:test';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
test('only the main window can invoke application commands', () => {
  const main = JSON.parse(read('src-tauri/capabilities/default.json'));
  const toast = JSON.parse(read('src-tauri/capabilities/toast.json'));
  assert.deepEqual(main.windows, ['main']); assert.deepEqual(toast.windows, ['toast']);
  assert.deepEqual(toast.permissions, ['core:event:allow-listen', 'core:event:allow-unlisten']);
  assert(main.permissions.includes('allow-save-settings'));
});
test('production CSP disallows external scripts and disables the broad asset protocol', () => {
  const security = JSON.parse(read('src-tauri/tauri.conf.json')).app.security;
  const scripts = security.csp.split(';').find(s => s.trim().startsWith('script-src'));
  assert.equal(scripts.trim(), "script-src 'self'");
  assert(!security.assetProtocol);
  assert(security.csp.includes("object-src 'none'"));
  assert(security.csp.includes("frame-src 'none'"));
});
test('every workflow action is immutable and only publishing gets repository write access', () => {
  for (const path of ['.github/workflows/build.yml', '.github/workflows/release.yml']) {
    const yaml = read(path);
    for (const [, ref] of yaml.matchAll(/uses: (\S+)/g)) assert.match(ref, /@[a-f0-9]{40}$/);
    assert.match(yaml, /\npermissions:\s*\n  contents: read/);
  }
  const release = read('.github/workflows/release.yml');
  assert.equal([...release.matchAll(/contents: write/g)].length, 1);
  assert.match(release, /publish:\s*\n    permissions:\s*\n      contents: write/);
});
