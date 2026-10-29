<script lang="ts">
  import { onMount } from 'svelte';
  import { textos } from '../lib/textos';
  let { consulta = $bindable(''), buscar }: { consulta: string; buscar: () => void } = $props();
  let input: HTMLInputElement;
  onMount(() => input?.focus());
</script>
<form class="buscador" onsubmit={e => { e.preventDefault(); buscar(); }} role="search">
  <label class="sr-only" for="consulta">{textos.buscar}</label>
  <div class="campo-busqueda"><span aria-hidden="true">⌕</span><input id="consulta" bind:this={input!} bind:value={consulta} placeholder={textos.buscarPlaceholder} autocomplete="off" spellcheck="false" /><button class="tecla" type="submit" aria-label={textos.buscar}>↵</button></div>
  <p class="ayuda">{textos.buscarAyuda}</p>
</form>
