import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import {
  detectMethodsFromSource,
  detectPageServerMethodsFromSource,
} from '../method-detector';

test('detects export const HTTP method', () => {
  const src = `export const GET = () => new Response('ok');`;
  assert.deepEqual(detectMethodsFromSource(src), ['GET']);
});

test('detects multiple methods across export styles', () => {
  const src = `
    export const GET = () => {};
    export async function POST(event) { return new Response(); }
    export function PUT() { return new Response(); }
    export let DELETE = () => {};
  `;
  const methods = detectMethodsFromSource(src);
  for (const m of ['GET', 'POST', 'PUT', 'DELETE']) {
    assert.ok(methods.includes(m as never), `${m} should be detected`);
  }
});

test('detects re-exports inside braces', () => {
  const src = `
    import { handleGet, handlePost } from './handlers';
    export { handleGet as GET, handlePost as POST };
  `;
  const methods = detectMethodsFromSource(src);
  assert.ok(methods.includes('GET' as never));
  assert.ok(methods.includes('POST' as never));
});

test('returns empty when no method exports are present', () => {
  const src = `
    export const helper = () => 42;
    export function utility() {}
  `;
  assert.deepEqual(detectMethodsFromSource(src), []);
});

test('ignores non-HTTP method exports', () => {
  const src = `
    export const config = { runtime: 'edge' };
    export const prerender = true;
    export function load() {}
  `;
  assert.deepEqual(detectMethodsFromSource(src), []);
});

test('detectPageServerMethods: load → GET, actions → POST', () => {
  assert.deepEqual(
    detectPageServerMethodsFromSource('export const load = () => ({});'),
    ['GET']
  );
  assert.deepEqual(
    detectPageServerMethodsFromSource('export const actions = { default: () => {} };'),
    ['POST']
  );
  const both = detectPageServerMethodsFromSource(`
    export const load = () => ({});
    export const actions = { default: () => {} };
  `);
  assert.ok(both.includes('GET' as never));
  assert.ok(both.includes('POST' as never));
});

test('detectPageServerMethods returns empty when neither is exported', () => {
  assert.deepEqual(detectPageServerMethodsFromSource('export const x = 1;'), []);
});

test('detects HEAD and OPTIONS exports', () => {
  const src = `
    export const HEAD = () => {};
    export async function OPTIONS() {}
  `;
  const methods = detectMethodsFromSource(src);
  assert.ok(methods.includes('HEAD' as never));
  assert.ok(methods.includes('OPTIONS' as never));
});
