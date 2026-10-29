<script lang="ts">
  import { textos } from '../lib/textos';
  import { hora, type Bloque as TipoBloque } from '../lib/api';
  import Markdown from './Markdown.svelte';
  import Codigo from './Codigo.svelte';
  let { bloque, abrir, detalle }: { bloque: TipoBloque; abrir: (id: string) => void; detalle: () => void } = $props();
</script>
{#if bloque.tipo === 'Usuario'}<div class="mensaje-usuario"><div class="burbuja"><Markdown texto={bloque.texto} /></div><span class="meta-mensaje">{textos.tu}{bloque.hora ? ` · ${hora(bloque.hora)}` : ''}</span></div>
{:else if bloque.tipo === 'Asistente'}<div class="mensaje-asistente"><div class="quien">{bloque.modelo || textos.asistente}<span>{hora(bloque.hora)}</span></div><Markdown texto={bloque.texto} /></div>
{:else if bloque.tipo === 'Herramientas'}<button class="resumen-herramientas" title={textos.verHerramientas} onclick={detalle}><span aria-hidden="true">⌘</span> {textos.herramientas(bloque.n, bloque.archivos)} <span aria-hidden="true">↗</span></button>
{:else if bloque.tipo === 'Herramienta'}<details class="detalle-evento" open><summary>{textos.herramienta}</summary><Codigo texto={bloque.texto} /></details>
{:else if bloque.tipo === 'Razonamiento'}<details class="detalle-evento"><summary><span aria-hidden="true">◇</span> {textos.razonamiento}</summary><Markdown texto={bloque.texto} /></details>
{:else if bloque.tipo === 'Subagente'}<button class="subagente" onclick={() => abrir(bloque.id_publico)} title={textos.leerSubagente}><span aria-hidden="true">↳</span> {textos.subagente}: {bloque.descripcion || bloque.id_publico} <span aria-hidden="true">↗</span></button>
{:else}<details class="detalle-evento sistema"><summary>{textos.sistema}</summary><Codigo texto={bloque.texto} /></details>{/if}
