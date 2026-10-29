<script lang="ts">
  import { onMount } from 'svelte';
  import { textos } from '../lib/textos';
  import { llamar, fecha, nombreFuente, type FilaSalud } from '../lib/api';
  let filas = $state<FilaSalud[]>([]);
  let aviso = $state<string | null>(null);
  let cargando = $state(false);
  let error = $state<string | null>(null);
  async function cargar() {
    cargando = true; error = null;
    try { [filas, aviso] = await Promise.all([llamar<FilaSalud[]>('salud'), llamar<string | null>('aviso_limpieza')]); }
    catch (e) { error = String(e); }
    finally { cargando = false; }
  }
  onMount(() => { void cargar(); });
</script>
<section class="panel-salud"><header><div><span class="etiqueta">{textos.salud}</span><h2>{textos.saludTitulo}</h2><p>{textos.saludDetalle}</p></div><button class="boton-secundario" disabled={cargando} onclick={cargar}>{cargando ? textos.cargando : textos.actualizar}</button></header>
  {#if error}<p class="error-lector" role="alert">{error}</p>{/if}
  {#each filas as fila}<section class="fuente-salud"><h3><span class:salud-error={fila.vistos !== fila.parseados + fila.excluidos}>{fila.vistos === fila.parseados + fila.excluidos ? '✓' : '✗'}</span>{nombreFuente(fila.fuente)}</h3><dl><div><dt>{textos.vistos}</dt><dd>{fila.vistos}</dd></div><div><dt>{textos.parseados}</dt><dd>{fila.parseados}</dd></div><div><dt>{textos.avisos}</dt><dd>{fila.avisos}</dd></div>{#if fila.excluidos}<div><dt>{textos.excluidos}</dt><dd>{fila.excluidos}</dd></div>{/if}</dl><p class="ultima-salud">{textos.ultima}: {fecha(fila.ultima)}</p>{#if fila.errores.length}<details class="errores-salud" open><summary>{textos.errores} · {fila.errores.length}</summary><ul>{#each fila.errores as [ruta, mensaje]}<li><code>{ruta}</code><p>{mensaje}</p></li>{/each}</ul></details>{:else}<p class="sin-errores">{textos.sinErrores}</p>{/if}</section>{/each}
  {#if aviso}<aside class="aviso-salud" role="note">{aviso}</aside>{/if}
</section>
