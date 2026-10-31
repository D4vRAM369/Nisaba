<script lang="ts">
  import { textos } from '../lib/textos';
  import type { Filtros } from '../lib/api';
  let { filtros = $bindable(), proyectos }: { filtros: Filtros; proyectos: string[] } = $props();
</script>
<div class="casillas">
  <label><input type="checkbox" checked disabled /> {textos.conversacion}</label>
  <label><input type="checkbox" bind:checked={filtros.razonamiento} /> {textos.razonamiento}</label>
  <label><input type="checkbox" bind:checked={filtros.codigo} /> {textos.codigo}</label>
</div>
<div class="filtros">
  <details><summary>{textos.fuente}<span aria-hidden="true">⌄</span></summary><label class="sr-only" for="fuente">{textos.fuente}</label><select id="fuente" bind:value={filtros.fuente}><option value={undefined}>{textos.todas}</option><option value="claude-code">{textos.claude}</option><option value="codex">{textos.codex}</option><option value="claude-web">{textos.claudeWeb}</option><option value="chatgpt">{textos.chatgpt}</option></select></details>
  <details><summary>{textos.proyecto}<span aria-hidden="true">⌄</span></summary><label class="sr-only" for="proyecto">{textos.proyecto}</label><select id="proyecto" bind:value={filtros.proyecto}><option value={undefined}>{textos.todos}</option>{#each proyectos as proyecto}<option value={proyecto}>{proyecto}</option>{/each}</select></details>
  <details><summary>{textos.fecha}<span aria-hidden="true">⌄</span></summary><label for="desde">{textos.desde}</label><input id="desde" type="date" value={filtros.desde ?? ''} onchange={e => filtros.desde = e.currentTarget.value || undefined} /></details>
</div>
{#if filtros.fuente || filtros.proyecto || filtros.desde}<button class="enlace limpiar" onclick={() => { filtros.fuente = undefined; filtros.proyecto = undefined; filtros.desde = undefined; }}>{textos.limpiar}</button>{/if}
