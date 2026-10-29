import DOMPurify from 'dompurify';
import { Marked } from 'marked';
import hljs from 'highlight.js/lib/core';
import javascript from 'highlight.js/lib/languages/javascript';
import typescript from 'highlight.js/lib/languages/typescript';
import rust from 'highlight.js/lib/languages/rust';
import python from 'highlight.js/lib/languages/python';
import bash from 'highlight.js/lib/languages/bash';
import json from 'highlight.js/lib/languages/json';
import sql from 'highlight.js/lib/languages/sql';
import { enlacePermitido } from './politica.mjs';
import { textos } from './textos';

for (const [nombre, lenguaje] of Object.entries({ javascript, typescript, rust, python, bash, json, sql })) hljs.registerLanguage(nombre, lenguaje);
export function escapar(s: string): string { return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;'); }
export function codigoHtml(texto: string, lenguaje = ''): string {
  return lenguaje && hljs.getLanguage(lenguaje) ? hljs.highlight(texto, { language: lenguaje }).value : escapar(texto);
}
const markdown = new Marked({ gfm: true, breaks: false, renderer: {
  html({ text }) { return escapar(text); },
  image({ text }) { return `<span class="imagen-omitida">[${escapar(text || textos.imagenBloqueada)}]</span>`; },
  link({ href, title, tokens }) {
    const texto = this.parser.parseInline(tokens);
    return enlacePermitido(href) ? `<a href="${escapar(href)}" target="_blank" rel="noreferrer noopener"${title ? ` title="${escapar(title)}"` : ''}>${texto}</a>` : texto;
  },
  code({ text, lang }) {
    const lenguaje = lang?.split(/\s/)[0] || '';
    return `<div class="caja-codigo"><div class="cabecera-codigo"><span>${escapar(lenguaje || textos.textoPlano)}</span><button type="button" data-copiar="codigo">${textos.copiar} ⧉</button></div><pre><code>${codigoHtml(text, lenguaje)}</code></pre></div>`;
  },
} });
export function sanear(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS: ['p', 'br', 'hr', 'strong', 'em', 'del', 's', 'blockquote', 'ul', 'ol', 'li', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'a', 'pre', 'code', 'span', 'div', 'button'], ALLOWED_ATTR: ['href', 'title', 'target', 'rel', 'class', 'type', 'data-copiar'], ALLOW_DATA_ATTR: false });
}
export function renderizar(texto: string): string { return sanear(markdown.parse(texto, { async: false })); }
