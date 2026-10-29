import { mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { mount } from 'svelte';
import App from '../src/App.svelte';
import type { Bloque, Lectura, Resultado } from '../src/lib/api';
import '../src/lib/tema.css';
import '@fontsource/cormorant-garamond/500.css';
import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/noto-sans-cuneiform/400.css';

// Respuestas inventadas del IPC real: nunca se incluyen en la aplicación compilada.
const resultados: Resultado[] = [
  { id_publico: 'cc-demo0001', titulo: 'Nisaba · Una memoria que no se pierde', fuente: 'claude-code', proyecto: 'Nisaba', fin: '2026-10-29T10:36:00Z', coincidencias: 7, fragmento: 'La «tablilla» conserva cada conversación y permite volver a las decisiones que tomamos.', orden_mensaje: 2, cwd: '/home/demo/Proyectos/Nisaba', id_original: 'demo0001-0000-4000-8000-000000000001' },
  { id_publico: 'cx-demo0002', titulo: 'Mirlo · Subtítulos y pequeñas decisiones', fuente: 'codex', proyecto: 'Mirlo', fin: '2026-10-26T18:22:00Z', coincidencias: 3, fragmento: 'Buscamos una forma de guardar el contexto en la «tablilla», sin depender de servicios externos.', orden_mensaje: 0, cwd: '/home/demo/Proyectos/Mirlo', id_original: '019a-demo0002' },
];
const mensajes: Bloque[] = [
  { tipo: 'Usuario', texto: 'Quiero poder volver a mis conversaciones sin perder el hilo. ¿Cómo organizamos la tablilla?', hora: '2026-10-29T10:30:00Z' },
  { tipo: 'Razonamiento', texto: 'Primero separo la conservación del formato de lectura. Así la tablilla mantiene todo el contenido y la interfaz muestra lo importante.', desplegado: false },
  { tipo: 'Asistente', modelo: 'Claude · Opus', hora: '2026-10-29T10:31:00Z', texto: 'La **tablilla** puede conservarlo todo y seguir siendo un lugar tranquilo para leer.\n\nTres decisiones nos ayudan:\n\n- Guardar una copia local antes de interpretar cada archivo.\n- Agrupar los resultados por conversación.\n- Dejar las herramientas en una sola línea.\n\n| Capa | Responsabilidad |\n| --- | --- |\n| Núcleo | Conservar y buscar |\n| Interfaz | Leer sin ruido |\n\n```rust\nlet recuerdo = tablilla.buscar("una idea");\nfor conversacion in recuerdo {\n    println!("{}", conversacion.titulo);\n}\n```' },
  { tipo: 'Herramientas', n: 4, archivos: 2 },
  { tipo: 'Usuario', texto: '¿Y si el archivo original desaparece?', hora: '2026-10-29T10:34:00Z' },
  { tipo: 'Asistente', modelo: 'Claude · Opus', hora: '2026-10-29T10:35:00Z', texto: 'El índice se conserva. La tablilla **solo crece**; borrar una fuente no borra lo que ya recordamos.\n\nEl panel de salud deja cualquier incidencia a la vista.' },
  { tipo: 'Subagente', id_publico: 'cc-hijo0001', descripcion: 'Revisión de conservación' },
];
// Estado en memoria de las exclusiones inventadas.
let excluidas: string[] = ['/home/demo/Proyectos/Privado'];
mockIPC((cmd, args: any) => {
  if (cmd === 'origen') return args.idPublico === 'cc-hijo0001'
    ? { cwd: '/home/demo/Proyectos/Nisaba', continuar: null }
    : { cwd: '/home/demo/Proyectos/Nisaba', continuar: 'cd "/home/demo/Proyectos/Nisaba" && claude --resume demo0001-0000-4000-8000-000000000001' };
  if (cmd === 'abrir_carpeta') return null;
  if (cmd === 'documento') {
    const secretos = new URLSearchParams(location.search).has('secreto') ? [{ tipo: 'Anthropic', muestra: 'sk-ant…a3F9' }] : [];
    return { markdown: `# ${resultados[0].titulo}\n\n- **Fuente:** claude-code\n\n---\n\n${mensajes.filter(b => 'texto' in b).map(b => (b as any).texto).join('\n\n')}`, nombre_md: '2026-10-29_nisaba_una-memoria.md', nombre_pdf: '2026-10-29_nisaba_una-memoria.pdf', secretos };
  }
  if (cmd === 'guardar_como') return `/home/demo/Documentos/${args.nombre}`;
  if (cmd === 'exclusiones_lista') return excluidas;
  if (cmd === 'proyectos_con_carpeta') return [['Mirlo', '/home/demo/Proyectos/Mirlo'], ['Nisaba', '/home/demo/Proyectos/Nisaba']];
  if (cmd === 'contar_en_carpeta') return args.carpeta.endsWith('Mirlo') ? 12 : 0;
  if (cmd === 'excluir') { excluidas = [...excluidas, args.carpeta]; return 0; }
  if (cmd === 'quitar_exclusion') { excluidas = excluidas.filter(c => c !== args.carpeta); return null; }
  if (cmd === 'elegir_carpeta') return '/home/demo/Descargas';
  if (cmd === 'iniciar' || cmd === 'estado_indexacion') {
    if (new URLSearchParams(location.search).has('indexando')) {
      setTimeout(() => void emit('indexacion', { estado: 'hecho', vistos: 708, total: 708 }), 5000);
      return { estado: 'indexando', vistos: 120, total: 708 };
    }
    return { estado: 'hecho', vistos: 708, total: 708 };
  }
  if (cmd === 'apertura_inicial') return new URLSearchParams(location.search).get('abrir');
  if (cmd === 'proyectos') return ['Mirlo', 'Nisaba'];
  if (cmd === 'aviso_limpieza') return 'Claude Code borra sesiones de más de 30 días. Sube cleanupPeriodDays en ~/.claude/settings.json.';
  if (cmd === 'salud') return [
    { fuente: 'claude-code', vistos: 160, parseados: 158, avisos: 0, excluidos: 2, ultima: '2026-10-29T10:36:00Z', errores: [] },
    { fuente: 'codex', vistos: 550, parseados: 549, avisos: 1, excluidos: 0, ultima: '2026-10-28T13:00:00Z', errores: [['/datos/inventados/rollout.jsonl', 'encogió: 20→8 mensajes; la versión anterior se conserva']] },
  ];
  if (cmd === 'recientes') {
    const params = new URLSearchParams(location.search);
    const todas = params.has('muchas') || params.has('parcial') ? Array.from({ length: 72 }, (_, i) => ({ ...resultados[i % 2], id_publico: i < 2 ? resultados[i].id_publico : `cc-prueba${i}`, titulo: `${resultados[i % 2].titulo} · ${i + 1}` })) : resultados;
    const r = params.has('vacia') ? [] : todas.filter(r => (!args.filtros.fuente || args.filtros.fuente === r.fuente) && (!args.filtros.proyecto || args.filtros.proyecto === r.proyecto) && (!args.filtros.desde || (r.fin && r.fin >= args.filtros.desde))).map(r => ({ ...r, coincidencias: 0, fragmento: 'Primera pregunta de esta conversación inventada.', orden_mensaje: 0 }));
    const desde = args.desplazamiento ?? 0;
    return { resultados: r.slice(desde, desde + (args.filtros.limite ?? 30)), total_sesiones: r.length, sugerencia: null };
  }
  if (cmd === 'buscar') {
    const consulta = args.consulta.toLowerCase();
    const r = consulta.includes('sinresultado') ? [] : resultados.filter(r => (!args.filtros.fuente || args.filtros.fuente === r.fuente) && (!args.filtros.proyecto || args.filtros.proyecto === r.proyecto));
    const respuesta = { resultados: r, total_sesiones: r.length, sugerencia: r.length ? null : 'Sin resultados. ¿Probar «memory»?' };
    return consulta.includes('lenta') ? new Promise(resolve => setTimeout(() => resolve(respuesta), 700)) : respuesta;
  }
  if (cmd === 'leer') {
    if (args.idPublico === 'cc-inexistente') throw new Error('No encuentro la conversación cc-inexistente');
    let bloques: Bloque[] = args.idPublico === 'cc-hijo0001' ? [{ tipo: 'Asistente', modelo: 'Revisor', hora: null, texto: 'La copia original se conserva correctamente.' }] : [...mensajes];
    if (new URLSearchParams(location.search).has('larga') && args.idPublico !== 'cc-hijo0001') bloques = Array.from({ length: 7 }, () => mensajes).flat();
    if (args.todo) bloques = bloques.flatMap(b => b.tipo === 'Herramientas' ? [{ tipo: 'Herramienta', texto: 'Read: {"file_path":"/tmp/ejemplo.rs"}\nResultado: contenido inventado' } as Bloque] : [b]);
    const desde = args.desde ?? 0;
    return { titulo: args.idPublico === 'cc-hijo0001' ? 'Revisión de conservación' : resultados[0].titulo, id_publico: args.idPublico, total: bloques.length, desde, bloques: bloques.slice(desde, desde + (args.cantidad ?? 20)), coincidencia: args.alrededorDe === undefined ? null : 2, cwd: '/home/demo/Proyectos/Nisaba', id_original: 'demo0001-0000-4000-8000-000000000001', fuente: 'claude-code', es_subagente: args.idPublico === 'cc-hijo0001' } satisfies Lectura;
  }
  throw new Error(`Comando de prueba no implementado: ${cmd}`);
}, { shouldMockEvents: true });
// isTauri usa este marcador; mockIPC solo sustituye el transporte.
Object.defineProperty(window, 'isTauri', { value: true, configurable: true });
mount(App, { target: document.getElementById('app')! });
