<script lang="ts">
  import { textos } from '../lib/textos';
  import {
    detallesWeb,
    versionesConversacion,
    fecha,
    atributosIframeHtml,
    type DetallesWeb,
    type Version,
  } from '../lib/api';
  import { enlacePermitido } from '../lib/politica.mjs';
  import Codigo from './Codigo.svelte';

  let {
    idPublico,
    versionActiva,
    hojaActiva,
    abrir,
    elegirVersion,
    elegirRama,
    volverActual,
  }: {
    idPublico: string;
    versionActiva: number | null;
    hojaActiva: string | null;
    abrir: (id: string) => void;
    elegirVersion: (v: number) => void;
    elegirRama: (hojaId: string) => void;
    volverActual: () => void;
  } = $props();

  let detalles = $state<DetallesWeb | null>(null);
  let versiones = $state<Version[]>([]);
  let cargando = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    const id = idPublico;
    cargando = true;
    error = null;
    detalles = null;
    versiones = [];
    void Promise.all([detallesWeb(id), versionesConversacion(id)])
      .then(([d, v]) => {
        if (id !== idPublico) return;
        detalles = d;
        versiones = v;
      })
      .catch(e => {
        if (id === idPublico) error = String(e);
      })
      .finally(() => {
        if (id === idPublico) cargando = false;
      });
  });

  function formatearBytes(n: number | null): string {
    if (n === null || n === undefined) return '';
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }
</script>

<aside class="panel-detalles-web" aria-label={textos.detallesWeb}>
  <header class="cabecera-detalles-web">
    <h3>{textos.detallesWeb}</h3>
    {#if versionActiva !== null || hojaActiva !== null}
      <button type="button" class="enlace" onclick={volverActual}>
        {textos.volverVersionActual}
      </button>
    {/if}
  </header>

  {#if cargando}
    <p class="aviso-contenido">{textos.cargando}</p>
  {:else if error}
    <p class="error-lector" role="alert">{error}</p>
  {:else if detalles}
    {@const tieneContenido =
      Boolean(detalles.html) ||
      Boolean(detalles.proyecto) ||
      detalles.documentos_proyecto.length > 0 ||
      detalles.adjuntos.length > 0 ||
      detalles.citas.length > 0 ||
      detalles.herramientas.length > 0 ||
      detalles.ramas.length > 1 ||
      versiones.length > 0}

    {#if !tieneContenido}
      <p class="aviso-contenido">{textos.sinDetallesWeb}</p>
    {/if}

    {#if detalles.html}
      {@const iframeAttrs = atributosIframeHtml(detalles.html, textos.vistaPreviaHtml)}
      <section class="bloque-detalle-web">
        <h4>🖼 {textos.vistaPreviaHtml}</h4>
        <iframe
          class="iframe-html-seguro"
          sandbox=""
          srcdoc={iframeAttrs.srcdoc}
          referrerpolicy="no-referrer"
          title={iframeAttrs.title}
        ></iframe>
      </section>
    {/if}

    {#if detalles.proyecto || detalles.documentos_proyecto.length > 0}
      <section class="bloque-detalle-web">
        <h4>🗂 {textos.proyectoYDocumentos}</h4>
        {#if detalles.proyecto}
          <p class="nombre-proyecto-web"><strong>{detalles.proyecto}</strong></p>
        {/if}
        {#if detalles.documentos_proyecto.length > 0}
          <ul class="lista-detalle-web">
            {#each detalles.documentos_proyecto as doc}
              <li>
                <button
                  type="button"
                  class="enlace"
                  class:activo={doc.id_publico === idPublico}
                  onclick={() => abrir(doc.id_publico)}
                >
                  {doc.titulo} <code>({doc.id_publico})</code>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}

    {#if detalles.adjuntos.length > 0}
      <section class="bloque-detalle-web">
        <h4>📎 {textos.adjuntosTitulo} ({detalles.adjuntos.length})</h4>
        <ul class="lista-detalle-web">
          {#each detalles.adjuntos as adj}
            <li class="item-adjunto-web">
              <div class="meta-adjunto-web">
                <strong>{adj.nombre}</strong>
                <span>
                  {[adj.tipo, formatearBytes(adj.tamano)].filter(Boolean).join(' · ')}
                </span>
              </div>
              {#if adj.sin_binario}
                <small class="nota-sin-binario">{textos.avisoAdjuntoSinBinario}</small>
              {/if}
              {#if adj.texto_extraido}
                <details class="detalle-evento">
                  <summary>{textos.verTextoExtraido}</summary>
                  <Codigo texto={adj.texto_extraido} />
                </details>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if detalles.citas.length > 0}
      <section class="bloque-detalle-web">
        <h4>🔗 {textos.citasTitulo} ({detalles.citas.length})</h4>
        <ol class="lista-citas-web">
          {#each detalles.citas as cita}
            <li>
              <span class="indice-cita">[{cita.indice}]</span>
              {#if enlacePermitido(cita.url)}
                <a href={cita.url} target="_blank" rel="noreferrer noopener">{cita.titulo || cita.url}</a>
              {:else}
                <span>{cita.titulo || cita.url}</span>
              {/if}
            </li>
          {/each}
        </ol>
      </section>
    {/if}

    {#if detalles.herramientas.length > 0}
      <section class="bloque-detalle-web">
        <h4>🛠 {textos.herramientasWebTitulo} ({detalles.herramientas.length})</h4>
        <div class="lista-herramientas-web">
          {#each detalles.herramientas as h}
            <details class="detalle-evento">
              <summary><code>{h.nombre}</code></summary>
              <Codigo texto={h.salida ? `${h.entrada}\n\nResultado:\n${h.salida}` : h.entrada} />
            </details>
          {/each}
        </div>
      </section>
    {/if}

    {#if detalles.ramas.length > 1 || versiones.length > 0}
      <section class="bloque-detalle-web">
        <h4>🕘 {textos.versionesYRamas}</h4>
        {#if detalles.ramas.length > 1}
          <p class="subtitulo-detalle-web">{textos.ramasConversacion}</p>
          <ul class="lista-detalle-web">
            {#each detalles.ramas as rama, idx}
              <li>
                <button
                  type="button"
                  class="boton-rama-web"
                  class:activo={(hojaActiva === null && rama.activa) || hojaActiva === rama.hoja_id}
                  onclick={() => (rama.activa ? volverActual() : elegirRama(rama.hoja_id))}
                >
                  <span>
                    {rama.activa ? textos.ramaActiva : `Rama #${idx + 1}`} · {rama.mensajes} msj
                  </span>
                  <small>{fecha(rama.fin)}</small>
                </button>
              </li>
            {/each}
          </ul>
        {/if}

        {#if versiones.length > 0}
          <p class="subtitulo-detalle-web">{textos.versionesAnteriores}</p>
          <ul class="lista-detalle-web">
            <li>
              <button
                type="button"
                class="boton-rama-web"
                class:activo={versionActiva === null && hojaActiva === null}
                onclick={volverActual}
              >
                <span>{textos.versionActual}</span>
              </button>
            </li>
            {#each versiones as v}
              <li>
                <button
                  type="button"
                  class="boton-rama-web"
                  class:activo={versionActiva === v.version}
                  onclick={() => elegirVersion(v.version)}
                >
                  <span>v{v.version} · {v.mensajes} msj</span>
                  <small>{fecha(v.importada_en)}</small>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  {/if}
</aside>
