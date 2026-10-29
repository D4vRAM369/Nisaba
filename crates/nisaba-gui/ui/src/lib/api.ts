import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { textos } from './textos';
import { formatearFecha } from './politica.mjs';

export type Fuente = 'claude-code' | 'codex';
export type Filtros = { fuente?: string; proyecto?: string; desde?: string; razonamiento: boolean; codigo: boolean; limite?: number };
export type Resultado = { id_publico: string; titulo: string; fuente: Fuente; proyecto: string | null; fin: string | null; coincidencias: number; fragmento: string; orden_mensaje: number; cwd: string | null; id_original: string };
export type Respuesta = { resultados: Resultado[]; total_sesiones: number; sugerencia: string | null };
export type EstadoIndexacion = { estado: 'indexando' | 'hecho' | 'ocupado' | 'fallo'; vistos: number; total: number; mensaje?: string | null };
export type Bloque =
  | { tipo: 'Usuario'; texto: string; hora: string | null }
  | { tipo: 'Asistente'; texto: string; modelo: string | null; hora: string | null }
  | { tipo: 'Herramientas'; n: number; archivos: number }
  | { tipo: 'Herramienta' | 'Sistema'; texto: string }
  | { tipo: 'Razonamiento'; texto: string; desplegado: boolean }
  | { tipo: 'Subagente'; id_publico: string; descripcion: string };
export type Lectura = { titulo: string; id_publico: string; total: number; desde: number; bloques: Bloque[]; coincidencia: number | null; cwd: string | null; id_original: string; fuente: Fuente; es_subagente: boolean };
export type FilaSalud = { fuente: Fuente; ultima: string | null; vistos: number; parseados: number; errores: [string, string][]; avisos: number; excluidos: number };
export type Hallazgo = { tipo: string; muestra: string };
export type Documento = { markdown: string; nombre_md: string; nombre_pdf: string; secretos: Hallazgo[] };
export type Origen = { cwd: string | null; continuar: string | null };
export const escritorio = () => isTauri();
export function llamar<T>(comando: string, args?: Record<string, unknown>): Promise<T> {
  return escritorio() ? invoke<T>(comando, args) : Promise.reject(new Error(textos.requiereApp));
}
export async function escuchar<T>(evento: string, recibir: (payload: T) => void): Promise<UnlistenFn> {
  return escritorio() ? listen<T>(evento, e => recibir(e.payload)) : () => {};
}
export const buscar = (consulta: string, filtros: Filtros) => llamar<Respuesta>('buscar', { consulta, filtros });
export const leer = (idPublico: string, opciones: { desde?: number; cantidad?: number; alrededorDe?: number; todo?: boolean }) => llamar<Lectura>('leer', { idPublico, ...opciones });
/** Portapapeles con respaldo para webviews sin la API asíncrona. */
export async function copiarTexto(texto: string): Promise<void> {
  try { await navigator.clipboard.writeText(texto); return; } catch { /* respaldo abajo */ }
  const area = Object.assign(document.createElement('textarea'), { value: texto });
  document.body.append(area); area.select();
  const ok = document.execCommand('copy'); area.remove();
  if (!ok) throw new Error(textos.errorCopiar);
}
export const nombreFuente = (fuente: Fuente) => fuente === 'claude-code' ? textos.claude : textos.codex;
export const fecha = (valor: string | null) => formatearFecha(valor, { day: 'numeric', month: 'short', year: 'numeric' }, textos.sinFecha);
export const hora = (valor: string | null) => formatearFecha(valor, { hour: '2-digit', minute: '2-digit' }, '');
