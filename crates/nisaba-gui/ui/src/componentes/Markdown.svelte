<script lang="ts">
  import { textos } from '../lib/textos';
  import { renderizar } from '../lib/markdown';
  import { acotarTexto } from '../lib/politica.mjs';
  let { texto }: { texto: string } = $props();
  let error = $state(false);
  let limite = $state(131072);
  let visible = $derived(acotarTexto(texto, limite));
  async function copiar(e: MouseEvent) {
    const boton = (e.target as Element).closest<HTMLButtonElement>('button[data-copiar]');
    if (!boton) return;
    const codigo = boton.closest('.caja-codigo')?.querySelector('code')?.textContent;
    if (codigo === undefined || codigo === null) return;
    try { await navigator.clipboard.writeText(codigo); boton.textContent = textos.copiado; error = false; }
    catch { error = true; }
  }
</script>
<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="markdown" onclick={copiar}>
  {#if texto.length > 131072}<p class="aviso-contenido">{textos.contenidoLargo}</p><pre class="texto-largo">{visible.contenido}</pre>{#if visible.recortado}<button class="mostrar-mas" onclick={() => limite += 131072}>{textos.mostrarMas}</button>{/if}
  {:else}{@html renderizar(texto)}{/if}
</div>
{#if error}<p role="alert" class="error-copia">{textos.errorCopiar}</p>{/if}
