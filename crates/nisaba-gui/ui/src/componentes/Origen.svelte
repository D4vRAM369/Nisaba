<script lang="ts">
  import { textos } from '../lib/textos';
  import { llamar, copiarTexto, type Origen } from '../lib/api';
  let { idPublico, noIndexar }: { idPublico: string; noIndexar: (carpeta: string) => void } = $props();
  let origen = $state<Origen | null>(null);
  let aviso = $state<string | null>(null);
  $effect(() => { const id = idPublico; origen = null; void llamar<Origen>('origen', { idPublico: id }).then(o => { if (id === idPublico) origen = o; }).catch(e => { if (id === idPublico) aviso = String(e); }); });
  let temporizador: ReturnType<typeof setTimeout> | undefined;
  function avisar(m: string) { aviso = m; clearTimeout(temporizador); temporizador = setTimeout(() => aviso = null, 2500); }
  async function copiar(texto: string, hecho: string) { try { await copiarTexto(texto); avisar(hecho); } catch (e) { avisar(String(e)); } }
  async function carpeta() { try { await llamar('abrir_carpeta', { idPublico }); } catch (e) { avisar(String(e)); } }
</script>
{#if origen && (origen.continuar || origen.cwd)}
  <div class="origen">
    {#if origen.continuar}<button class="boton-origen" title={textos.continuarTitulo} onclick={() => copiar(origen!.continuar!, textos.comandoCopiado)}>{textos.continuar}</button>{/if}
    {#if origen.cwd}
      <button class="boton-origen" title={textos.carpetaTitulo} onclick={carpeta}>{textos.carpeta}</button>
      <button class="boton-origen" title={textos.rutaTitulo} onclick={() => copiar(origen!.cwd!, textos.rutaCopiada)}>{textos.ruta}</button>
      <button class="enlace no-indexar" onclick={() => noIndexar(origen!.cwd!)}>{textos.noIndexarProyecto}</button>
    {/if}
    {#if aviso}<span class="aviso-breve" role="status">{aviso}</span>{/if}
  </div>
{/if}
