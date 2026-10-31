import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { enlacePermitido, acotarTexto, formatearFecha, esIdWeb, atributosIframeHtml } from '../src/lib/politica.mjs';

test('no permite esquemas ejecutables, datos ni rutas locales en enlaces', () => {
  for (const url of ['javascript:alert(1)', 'JaVaScRiPt:alert(1)', 'java\nscript:alert(1)', 'data:text/html,x', 'file:///etc/passwd', '//example.org', '/etc/passwd']) assert.equal(enlacePermitido(url), false, url);
  for (const url of ['https://example.org/docs', 'http://localhost:3000']) assert.equal(enlacePermitido(url), true, url);
});
test('acota el texto enorme sin descartar la indicación de contenido pendiente', () => {
  assert.deepEqual(acotarTexto('0123456789', 5), { contenido: '01234', recortado: true });
  assert.deepEqual(acotarTexto('abc', 5), { contenido: 'abc', recortado: false });
});

test('una fecha inválida o ausente muestra el texto alternativo sin lanzar', () => {
  for (const valor of [null, '', 'no-es-una-fecha', '2026-99-99']) {
    assert.equal(formatearFecha(valor, { year: 'numeric' }, 'Sin fecha'), 'Sin fecha');
    assert.equal(formatearFecha(valor, { hour: '2-digit', minute: '2-digit' }, ''), '');
  }
  assert.match(formatearFecha('2026-10-29T12:00:00Z', { year: 'numeric' }, 'Sin fecha'), /2026/);
});

test('detecta únicamente prefijos de conversaciones web (cw- y gpt-)', () => {
  assert.equal(esIdWeb('cw-12345678'), true);
  assert.equal(esIdWeb('gpt-abcdef12'), true);
  assert.equal(esIdWeb('cc-12345678'), false);
  assert.equal(esIdWeb('cx-12345678'), false);
  assert.equal(esIdWeb(null), false);
});

test('los documentos HTML en Detalles se renderizan en un iframe con sandbox vacío y srcdoc', () => {
  const attrs = atributosIframeHtml('<!doctype html><script>alert(1)</script><h1>Frame</h1>', 'Frame');
  assert.equal(attrs.sandbox, '');
  assert.equal(attrs.referrerpolicy, 'no-referrer');
  assert.match(attrs.srcdoc, /<h1>Frame<\/h1>/);

  const rutaDetalles = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '../src/componentes/Detalles.svelte',
  );
  const codigo = fs.readFileSync(rutaDetalles, 'utf8');
  assert.match(codigo, /<iframe[\s\S]*?sandbox=""[\s\S]*?srcdoc=/);
  assert.doesNotMatch(codigo, /allow-scripts|allow-same-origin/i);
});
