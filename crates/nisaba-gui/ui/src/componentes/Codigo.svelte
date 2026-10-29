<script lang="ts">
  import { textos } from '../lib/textos';
  import { codigoHtml } from '../lib/markdown';
  import { acotarTexto } from '../lib/politica.mjs';
  let { texto, lenguaje = '' }: { texto: string; lenguaje?: string } = $props();
  let copiado = $state(false);
  let fallo = $state(false);
  let limite = $state(131072);
  let visible = $derived(acotarTexto(texto, limite));
  async function copiar() {
    try { await navigator.clipboard.writeText(texto); copiado = true; fallo = false; }
    catch { fallo = true; }
  }
</script>
<div class="caja-codigo"><div class="cabecera-codigo"><span>{lenguaje || textos.textoPlano}</span><button onclick={copiar}>{copiado ? textos.copiado : textos.copiar} <span aria-hidden="true">⧉</span></button></div><pre><code>{@html codigoHtml(visible.contenido, lenguaje)}</code></pre>{#if visible.recortado}<button class="mostrar-mas" onclick={() => limite += 131072}>{textos.mostrarMas}</button>{/if}{#if fallo}<p role="alert" class="error-copia">{textos.errorCopiar}</p>{/if}</div>
