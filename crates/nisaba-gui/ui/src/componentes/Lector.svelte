<script lang="ts">
  import { tick } from 'svelte';
  import { textos } from '../lib/textos';
  import { leer, type Lectura } from '../lib/api';
  import Bloque from './Bloque.svelte';
  import Exportar from './Exportar.svelte';
  import Origen from './Origen.svelte';
  let { idPublico, alrededorDe, abrir, noIndexar }: { idPublico: string; alrededorDe?: number; abrir: (id: string) => void; noIndexar: (carpeta: string) => void } = $props();
  let lectura = $state<Lectura | null>(null);
  let todo = $state(false);
  let cargando = $state(false);
  let error = $state<string | null>(null);
  let cuerpo: HTMLDivElement;
  let secuencia = 0;
  async function cargar(desde?: number) {
    const actual = ++secuencia;
    const id = idPublico;
    cargando = true; error = null;
    try {
      const r = await leer(id, { cantidad: 20, desde, alrededorDe: desde === undefined ? alrededorDe : undefined, todo });
      if (actual !== secuencia) return;
      lectura = r;
      await tick();
      const ancla = cuerpo?.querySelector<HTMLElement>('[data-coincidencia="true"]');
      if (ancla) ancla.scrollIntoView({ block: 'center' }); else cuerpo?.scrollTo({ top: 0 });
    } catch (e) { if (actual === secuencia) { error = String(e); lectura = null; } }
    finally { if (actual === secuencia) cargando = false; }
  }
  $effect(() => { idPublico; alrededorDe; todo; void cargar(); });
</script>
<header class="cabecera-lector"><div><span class="etiqueta">{textos.soloConversacion}</span><h2>{lectura?.titulo || textos.cargando}</h2><span class="id-conversacion">{idPublico}</span><Origen {idPublico} {noIndexar} /></div><div class="acciones-lector"><label class="ver-todo"><input type="checkbox" bind:checked={todo} />{textos.verTodo}</label><Exportar {idPublico} {todo} /></div></header>
<div class="cuerpo-lector" bind:this={cuerpo!} aria-busy={cargando}>
  {#if error}<p class="error-lector" role="alert">{error}</p>{/if}
  {#if cargando && !lectura}<p class="aviso-contenido" role="status">{textos.cargando}</p>{/if}
  {#if lectura}
    {#if lectura.desde > 0}<button class="paginacion" disabled={cargando} onclick={() => cargar(Math.max(0, lectura!.desde - 20))}>↑ {textos.anteriores}</button>{/if}
    <div class="bloques">{#each lectura.bloques as bloque, i}<article class:coincidencia={lectura.coincidencia === lectura.desde + i} data-coincidencia={lectura.coincidencia === lectura.desde + i} aria-label={lectura.coincidencia === lectura.desde + i ? textos.coincidencia : undefined}><Bloque {bloque} {abrir} detalle={() => todo = true} /></article>{/each}</div>
    {#if lectura.desde + lectura.bloques.length < lectura.total}<button class="paginacion" disabled={cargando} onclick={() => cargar(lectura!.desde + lectura!.bloques.length)}>{textos.siguientes} ↓</button>{/if}
    <p class="contador-bloques">{textos.bloques(lectura.bloques.length ? lectura.desde + 1 : 0, lectura.desde + lectura.bloques.length, lectura.total)}</p>
  {/if}
</div>
