<script lang="ts">
  import { tick } from 'svelte';
  import { textos } from '../lib/textos';
  import { leer, leerVersion, leerRama, esIdWeb, nombreFuente, type Lectura } from '../lib/api';
  import Bloque from './Bloque.svelte';
  import Exportar from './Exportar.svelte';
  import Origen from './Origen.svelte';
  import Detalles from './Detalles.svelte';
  let { idPublico, alrededorDe, abrir, noIndexar }: { idPublico: string; alrededorDe?: number; abrir: (id: string) => void; noIndexar: (carpeta: string) => void } = $props();
  let lectura = $state<Lectura | null>(null);
  let todo = $state(false);
  let cargando = $state(false);
  let error = $state<string | null>(null);
  let mostrarDetalles = $state(true);
  let versionActiva = $state<number | null>(null);
  let hojaActiva = $state<string | null>(null);
  let cuerpo: HTMLDivElement;
  let secuencia = 0;
  const esWeb = $derived(esIdWeb(idPublico));
  async function cargar(desde?: number) {
    const actual = ++secuencia;
    const id = idPublico;
    const ver = versionActiva;
    const hoja = hojaActiva;
    cargando = true; error = null;
    try {
      const r = ver !== null
        ? await leerVersion(id, ver, { cantidad: 20, desde, todo })
        : hoja !== null
          ? await leerRama(id, hoja, { cantidad: 20, desde, todo })
          : await leer(id, { cantidad: 20, desde, alrededorDe: desde === undefined ? alrededorDe : undefined, todo });
      if (actual !== secuencia) return;
      lectura = r;
      await tick();
      const ancla = cuerpo?.querySelector<HTMLElement>('[data-coincidencia="true"]');
      if (ancla) ancla.scrollIntoView({ block: 'center' }); else cuerpo?.scrollTo({ top: 0 });
    } catch (e) { if (actual === secuencia) { error = String(e); lectura = null; } }
    finally { if (actual === secuencia) cargando = false; }
  }
  $effect(() => { idPublico; alrededorDe; todo; versionActiva; hojaActiva; void cargar(); });
</script>
<header class="cabecera-lector">
  <div>
    <span class="etiqueta">
      {lectura ? `${nombreFuente(lectura.fuente)} · ${textos.soloConversacion}` : textos.soloConversacion}
    </span>
    <h2>{lectura?.titulo || textos.cargando}</h2>
    <span class="id-conversacion">{idPublico}</span>
    {#if versionActiva !== null}
      <span class="insignia-version">{textos.viendoVersion(versionActiva)}</span>
    {:else if hojaActiva !== null}
      <span class="insignia-version">{textos.viendoRama}</span>
    {/if}
    <Origen {idPublico} {noIndexar} />
  </div>
  <div class="acciones-lector">
    <div class="controles-lector-fila">
      <label class="ver-todo"><input type="checkbox" bind:checked={todo} />{textos.verTodo}</label>
      {#if esWeb}
        <button type="button" class="boton-exportar" class:activo={mostrarDetalles} onclick={() => mostrarDetalles = !mostrarDetalles}>
          {mostrarDetalles ? textos.ocultarDetalles : textos.detallesWeb} ▤
        </button>
      {/if}
    </div>
    <Exportar {idPublico} {todo} />
  </div>
</header>
<div class="contenedor-lector" class:con-detalles={esWeb && mostrarDetalles}>
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
  {#if esWeb && mostrarDetalles}
    <Detalles
      {idPublico}
      {versionActiva}
      {hojaActiva}
      {abrir}
      elegirVersion={v => { versionActiva = v; hojaActiva = null; }}
      elegirRama={h => { hojaActiva = h; versionActiva = null; }}
      volverActual={() => { versionActiva = null; hojaActiva = null; }}
    />
  {/if}
</div>
