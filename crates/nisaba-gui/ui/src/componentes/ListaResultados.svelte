<script lang="ts">
  import { textos } from '../lib/textos';
  import { fecha, nombreFuente, type Respuesta, type Resultado } from '../lib/api';
  let { respuesta, seleccionado, consulta, pendiente, abrir, cargarMas }: { respuesta: Respuesta; seleccionado: string | null; consulta: string; pendiente: boolean; abrir: (r: Resultado) => void; cargarMas: () => void } = $props();
  const fragmentos = (s: string) => s.split(/(«[^»]*»)/g);
</script>
<div class="cabecera-lista"><h2>{textos.resultados}</h2><span>{pendiente ? textos.buscando : (respuesta.resultados.length < respuesta.total_sesiones ? textos.sesionesMostradas(respuesta.resultados.length, respuesta.total_sesiones) : textos.sesiones(respuesta.total_sesiones))}</span></div>
<div class="lista" aria-busy={pendiente}>
  {#if !pendiente && !respuesta.resultados.length}<div class="estado-vacio"><h3>{consulta.trim() ? textos.sinResultados : textos.sinConversaciones}</h3><p>{consulta.trim() ? textos.probarOtra : textos.sinConversacionesDetalle}</p></div>
  {:else}{#each respuesta.resultados as resultado}
    <button class="resultado" class:seleccionado={seleccionado === resultado.id_publico} aria-pressed={seleccionado === resultado.id_publico} onclick={() => abrir(resultado)}>
      <div class="resultado-meta"><span class="fuente"><i class={resultado.fuente} aria-hidden="true"></i>{nombreFuente(resultado.fuente)}</span><time>{fecha(resultado.fin)}</time></div>
      <h3>{resultado.titulo || textos.sinTitulo}</h3>
      <p class="fragmento">{#each fragmentos(resultado.fragmento) as parte}{#if parte.startsWith('«') && parte.endsWith('»')}<mark>{parte.slice(1, -1)}</mark>{:else}{parte}{/if}{/each}</p>
      <div class="resultado-pie"><span>{resultado.proyecto ?? ''}</span>{#if consulta.trim()}<span>{textos.coincidencias(resultado.coincidencias)}</span>{/if}</div>
    </button>
  {/each}{/if}
  {#if !consulta.trim() && respuesta.resultados.length < respuesta.total_sesiones}<button class="paginacion" disabled={pendiente} onclick={cargarMas}>{pendiente ? textos.cargando : textos.cargarMasConversaciones}</button>{/if}
  {#if respuesta.sugerencia}<p class="sugerencia" role="status">{respuesta.sugerencia}</p>
  {:else if consulta.trim() && respuesta.resultados.length < respuesta.total_sesiones}<p class="sugerencia" role="status">{textos.listadoParcial(respuesta.resultados.length, respuesta.total_sesiones)}</p>{/if}
</div>
