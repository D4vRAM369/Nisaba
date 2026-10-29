/** @param {string} url */
export function enlacePermitido(url) {
  if (/[\u0000-\u0020\u007f]/.test(url) || !/^https?:\/\//i.test(url)) return false;
  try { const valor = new URL(url); return Boolean(valor.hostname) && !valor.username && !valor.password; }
  catch { return false; }
}
/** @param {string} texto @param {number} [limite] */
export function acotarTexto(texto, limite = 131072) {
  return { contenido: texto.slice(0, limite), recortado: texto.length > limite };
}
/** @param {string|null} valor @param {Intl.DateTimeFormatOptions} opciones @param {string} alternativo */
export function formatearFecha(valor, opciones, alternativo) {
  if (!valor) return alternativo;
  const instante = new Date(valor);
  return Number.isNaN(instante.getTime()) ? alternativo : new Intl.DateTimeFormat('es', opciones).format(instante);
}
