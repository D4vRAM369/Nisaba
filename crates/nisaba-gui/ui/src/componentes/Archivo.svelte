<script lang="ts">
  import { onMount } from 'svelte';
  import { textos } from '../lib/textos';
  import {
    estadoArchivo,
    prepararArchivo,
    confirmarArchivo,
    desbloquearArchivo,
    bloquearArchivo,
    recuperarArchivo,
    cambiarContrasenaArchivo,
    importarZip,
    elegirZip,
    elegirCarpetaImportacion,
    copiarTexto,
    escuchar,
    type EstadoArchivo,
    type ResumenImportacion,
  } from '../lib/api';

  let {
    estado = $bindable('no_existe'),
    cambio,
  }: {
    estado: EstadoArchivo;
    cambio: (nuevo: EstadoArchivo) => void;
  } = $props();

  let cargando = $state(false);
  let error = $state<string | null>(null);
  let aviso = $state<string | null>(null);

  // Flujo 1: Crear archivo web
  let clave1 = $state('');
  let clave2 = $state('');
  let codigoGenerado = $state<string | null>(null);
  let casillaConfirmada = $state(false);
  let copiadoCodigo = $state(false);

  // Flujo 2: Desbloquear
  let claveDesbloqueo = $state('');

  // Flujo 3: Recuperar con código
  let modoRecuperar = $state(false);
  let codigoRecuperacion = $state('');
  let nuevaClaveRec1 = $state('');
  let nuevaClaveRec2 = $state('');

  // Flujo 4: Importar y cambiar contraseña
  let importando = $state(false);
  let arrastrando = $state(false);
  let resumen = $state<ResumenImportacion | null>(null);
  let claveActual = $state('');
  let claveNueva1 = $state('');
  let claveNueva2 = $state('');

  async function refrescarEstado() {
    try {
      const actual = await estadoArchivo();
      estado = actual;
      cambio(actual);
    } catch (e) {
      error = String(e);
    }
  }

  async function pasoPreparar(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    aviso = null;
    if ([...clave1].length < 12) {
      error = textos.contrasenaCorta;
      return;
    }
    if (clave1 !== clave2) {
      error = textos.contrasenasNoCoinciden;
      return;
    }
    cargando = true;
    try {
      codigoGenerado = await prepararArchivo(clave1, clave2);
      casillaConfirmada = false;
      copiadoCodigo = false;
    } catch (err) {
      error = String(err);
    } finally {
      cargando = false;
    }
  }

  async function pasoConfirmar() {
    error = null;
    aviso = null;
    cargando = true;
    try {
      await confirmarArchivo(casillaConfirmada);
      clave1 = '';
      clave2 = '';
      codigoGenerado = null;
      casillaConfirmada = false;
      await refrescarEstado();
    } catch (err) {
      error = String(err);
    } finally {
      cargando = false;
    }
  }

  async function copiarCodigo() {
    if (!codigoGenerado) return;
    try {
      await copiarTexto(codigoGenerado);
      copiadoCodigo = true;
    } catch (err) {
      error = String(err);
    }
  }

  async function hacerDesbloqueo(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    aviso = null;
    cargando = true;
    try {
      await desbloquearArchivo(claveDesbloqueo);
      claveDesbloqueo = '';
      await refrescarEstado();
    } catch (err) {
      error = String(err);
    } finally {
      cargando = false;
    }
  }

  async function hacerRecuperacion(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    aviso = null;
    if ([...nuevaClaveRec1].length < 12) {
      error = textos.contrasenaCorta;
      return;
    }
    if (nuevaClaveRec1 !== nuevaClaveRec2) {
      error = textos.contrasenasNoCoinciden;
      return;
    }
    cargando = true;
    try {
      await recuperarArchivo(codigoRecuperacion.trim(), nuevaClaveRec1, nuevaClaveRec2);
      codigoRecuperacion = '';
      nuevaClaveRec1 = '';
      nuevaClaveRec2 = '';
      modoRecuperar = false;
      aviso = textos.contrasenaRestablecida;
      await refrescarEstado();
    } catch (err) {
      error = String(err);
    } finally {
      cargando = false;
    }
  }

  async function hacerBloqueo() {
    error = null;
    aviso = null;
    resumen = null;
    try {
      await bloquearArchivo();
      await refrescarEstado();
    } catch (err) {
      error = String(err);
    }
  }

  async function ejecutarImportacion(ruta: string) {
    error = null;
    aviso = null;
    importando = true;
    try {
      resumen = await importarZip(ruta);
      cambio(estado);
    } catch (err) {
      error = String(err);
    } finally {
      importando = false;
    }
  }

  async function seleccionarCarpeta() {
    error = null;
    try {
      const ruta = await elegirCarpetaImportacion();
      if (ruta) await ejecutarImportacion(ruta);
    } catch (err) {
      error = String(err);
    }
  }

  async function seleccionarZip() {
    error = null;
    try {
      const ruta = await elegirZip();
      if (ruta) await ejecutarImportacion(ruta);
    } catch (err) {
      error = String(err);
    }
  }

  async function hacerCambioContrasena(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    aviso = null;
    if ([...claveNueva1].length < 12) {
      error = textos.contrasenaCorta;
      return;
    }
    if (claveNueva1 !== claveNueva2) {
      error = textos.contrasenasNoCoinciden;
      return;
    }
    cargando = true;
    try {
      await cambiarContrasenaArchivo(claveActual, claveNueva1, claveNueva2);
      claveActual = '';
      claveNueva1 = '';
      claveNueva2 = '';
      aviso = textos.contrasenaCambiada;
    } catch (err) {
      error = String(err);
    } finally {
      cargando = false;
    }
  }

  function etiquetaEstado(e: EstadoArchivo): string {
    if (e === 'desbloqueado') return textos.estadoDesbloqueado;
    if (e === 'bloqueado') return textos.estadoBloqueado;
    return textos.estadoSinCrear;
  }

  onMount(() => {
    void refrescarEstado();
    let activo = true;
    let desuscribir: (() => void) | null = null;
    void escuchar<{ paths?: string[] }>('tauri://drag-drop', payload => {
      arrastrando = false;
      if (!activo || estado !== 'desbloqueado' || importando) return;
      const primera = payload?.paths?.[0];
      if (primera) void ejecutarImportacion(primera);
    }).then(off => {
      if (!activo) off();
      else desuscribir = off;
    });
    return () => {
      activo = false;
      desuscribir?.();
    };
  });
</script>

<section class="panel-salud panel-archivo">
  <header>
    <div>
      <span class="etiqueta">{textos.archivoWeb} · {etiquetaEstado(estado)}</span>
      <h2>{textos.archivoWeb}</h2>
      <p>{textos.archivoSubtitulo}</p>
    </div>
    {#if estado === 'desbloqueado'}
      <button class="boton-secundario" onclick={hacerBloqueo}>{textos.bloquearAhora}</button>
    {/if}
  </header>

  {#if error}<p class="error-lector" role="alert">{error}</p>{/if}
  {#if aviso}<aside class="aviso-salud" role="status">{aviso}</aside>{/if}

  {#if estado === 'no_existe'}
    <div class="tarjeta-archivo">
      {#if !codigoGenerado}
        <h3>{textos.crearArchivoTitulo}</h3>
        <p class="detalle-archivo">{textos.crearArchivoDetalle}</p>
        <form class="formulario-archivo" onsubmit={pasoPreparar}>
          <label>
            <span>{textos.contrasena}</span>
            <input type="password" bind:value={clave1} minlength="12" required autocomplete="new-password" />
          </label>
          <label>
            <span>{textos.repetirContrasena}</span>
            <input type="password" bind:value={clave2} minlength="12" required autocomplete="new-password" />
          </label>
          <div class="acciones-formulario">
            <button type="submit" class="boton-primario" disabled={cargando}>
              {cargando ? textos.cargando : textos.generarCodigo}
            </button>
          </div>
        </form>
      {:else}
        <h3>{textos.codigoRecuperacionTitulo}</h3>
        <p class="detalle-archivo">{textos.codigoRecuperacionDetalle}</p>
        <div class="caja-codigo-recuperacion">
          <code>{codigoGenerado}</code>
          <button type="button" class="boton-secundario" onclick={copiarCodigo}>
            {copiadoCodigo ? textos.copiado : textos.copiarCodigo}
          </button>
        </div>
        <aside class="aviso-salud">{textos.avisoCopiaSeguridad}</aside>
        <label class="casilla-obligatoria">
          <input type="checkbox" bind:checked={casillaConfirmada} />
          <span>{textos.casillaCodigoGuardado}</span>
        </label>
        <div class="acciones-formulario">
          <button
            type="button"
            class="boton-primario"
            disabled={!casillaConfirmada || cargando}
            onclick={pasoConfirmar}
          >
            {cargando ? textos.cargando : textos.confirmarCrearArchivo}
          </button>
        </div>
      {/if}
    </div>
  {:else if estado === 'bloqueado'}
    <div class="tarjeta-archivo">
      {#if !modoRecuperar}
        <h3>{textos.desbloquearTitulo}</h3>
        <p class="detalle-archivo">{textos.desbloquearDetalle}</p>
        <form class="formulario-archivo" onsubmit={hacerDesbloqueo}>
          <label>
            <span>{textos.contrasena}</span>
            <input type="password" bind:value={claveDesbloqueo} required autocomplete="current-password" />
          </label>
          <div class="acciones-formulario">
            <button type="submit" class="boton-primario" disabled={cargando}>
              {cargando ? textos.cargando : textos.desbloquearBoton}
            </button>
            <button type="button" class="enlace" onclick={() => { modoRecuperar = true; error = null; }}>
              {textos.olvideContrasena}
            </button>
          </div>
        </form>
      {:else}
        <h3>{textos.recuperarTitulo}</h3>
        <form class="formulario-archivo" onsubmit={hacerRecuperacion}>
          <label>
            <span>{textos.codigoRecuperacionLabel}</span>
            <input type="text" bind:value={codigoRecuperacion} placeholder="XXXX-XXXX-XXXX-XXXX-…" required />
          </label>
          <label>
            <span>{textos.nuevaContrasena}</span>
            <input type="password" bind:value={nuevaClaveRec1} minlength="12" required autocomplete="new-password" />
          </label>
          <label>
            <span>{textos.repetirNuevaContrasena}</span>
            <input type="password" bind:value={nuevaClaveRec2} minlength="12" required autocomplete="new-password" />
          </label>
          <div class="acciones-formulario">
            <button type="submit" class="boton-primario" disabled={cargando}>
              {cargando ? textos.cargando : textos.recuperarBoton}
            </button>
            <button type="button" class="boton-secundario" onclick={() => { modoRecuperar = false; error = null; }}>
              {textos.cancelar}
            </button>
          </div>
        </form>
      {/if}
    </div>
  {:else if estado === 'desbloqueado'}
    <div
      class="tarjeta-archivo zona-importacion"
      class:arrastrando
      role="region"
      aria-label={textos.importarTitulo}
      ondragover={e => { e.preventDefault(); arrastrando = true; }}
      ondragleave={() => { arrastrando = false; }}
      ondrop={e => { e.preventDefault(); arrastrando = false; }}
    >
      <h3>{textos.importarTitulo}</h3>
      <p class="detalle-archivo">{textos.importarDetalle}</p>
      <div class="botones-importar">
        <button type="button" class="boton-primario" disabled={importando} onclick={seleccionarCarpeta}>
          {textos.elegirCarpetaExportacion}
        </button>
        <button type="button" class="boton-secundario" disabled={importando} onclick={seleccionarZip}>
          {textos.elegirArchivoZip}
        </button>
      </div>
      <p class="nota-arrastre">{importando ? textos.importando : textos.zonaArrastre}</p>

      {#if resumen}
        <div class="resumen-importacion" role="status">
          <p class="resumen-linea">
            <strong>
              {textos.resumenImportacion(
                textos.archivoWeb,
                resumen.nuevas,
                resumen.actualizadas,
                resumen.versionadas,
                resumen.iguales
              )}
            </strong>
          </p>
          {#if resumen.avisos.length}
            <ul class="avisos-importacion">
              {#each resumen.avisos as av}<li>{av}</li>{/each}
            </ul>
          {/if}
          <p class="aviso-zip-sin-cifrar">{textos.avisoZipSinCifrar(resumen.ruta_origen)}</p>
        </div>
      {/if}
    </div>

    <aside class="aviso-salud">{textos.avisoCopiaSeguridad}</aside>

    <details class="tarjeta-archivo cambiar-clave-panel">
      <summary>{textos.cambiarContrasenaTitulo}</summary>
      <form class="formulario-archivo" onsubmit={hacerCambioContrasena}>
        <label>
          <span>{textos.contrasenaActual}</span>
          <input type="password" bind:value={claveActual} required autocomplete="current-password" />
        </label>
        <label>
          <span>{textos.nuevaContrasena}</span>
          <input type="password" bind:value={claveNueva1} minlength="12" required autocomplete="new-password" />
        </label>
        <label>
          <span>{textos.repetirNuevaContrasena}</span>
          <input type="password" bind:value={claveNueva2} minlength="12" required autocomplete="new-password" />
        </label>
        <div class="acciones-formulario">
          <button type="submit" class="boton-secundario" disabled={cargando}>
            {cargando ? textos.cargando : textos.cambiarContrasenaBoton}
          </button>
        </div>
      </form>
    </details>
  {/if}
</section>
