import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import { detectMethodsFromSource } from '../method-detector';

test('UI-only route defaults to GET', () => {
  const r = detectMethodsFromSource('export default function Page(){ return null; }');
  assert.deepEqual(r.methods, ['GET']);
  assert.equal(r.hasLoader, false);
  assert.equal(r.hasAction, false);
});

test('loader → GET', () => {
  const r = detectMethodsFromSource('export const loader = async () => null;');
  assert.deepEqual(r.methods, ['GET']);
  assert.equal(r.hasLoader, true);
});

test('action → POST/PUT/PATCH/DELETE', () => {
  const r = detectMethodsFromSource('export const action = async () => null;');
  assert.deepEqual(r.methods.sort(), ['DELETE', 'PATCH', 'POST', 'PUT']);
});

test('loader + action → union', () => {
  const r = detectMethodsFromSource(`
    export async function loader() { return null }
    export async function action() { return null }
  `);
  assert.deepEqual(r.methods, ['GET', 'POST', 'PUT', 'PATCH', 'DELETE']);
});

test('various export styles detected', () => {
  for (const src of [
    'export const loader = () => null;',
    'export let loader = () => null;',
    'export var loader = () => null;',
    'export function loader() { return null }',
    'export async function loader() { return null }',
  ]) {
    assert.equal(detectMethodsFromSource(src).hasLoader, true, `failed for: ${src}`);
  }
});

test('re-export bracket form', () => {
  const r = detectMethodsFromSource(`
    function l() {}
    function a() {}
    export { l as loader, a as action };
  `);
  assert.equal(r.hasLoader, true);
  assert.equal(r.hasAction, true);
});

test('commented-out exports do not count', () => {
  const r = detectMethodsFromSource(`
    // export const loader = () => null;
    /* export async function action() {} */
    export default function Page() {}
  `);
  assert.equal(r.hasLoader, false);
  assert.equal(r.hasAction, false);
  assert.deepEqual(r.methods, ['GET']);
});

test('http:// in code is not mistaken for a comment', () => {
  const r = detectMethodsFromSource(`
    const url = 'http://example.com/x';
    export const loader = () => fetch(url);
  `);
  assert.equal(r.hasLoader, true);
});
