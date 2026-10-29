<script lang="ts">
  import { textos } from '../lib/textos';
  import type { EstadoIndexacion } from '../lib/api';
  let { estado }: { estado: EstadoIndexacion } = $props();
  let aviso = $derived(estado.estado === 'fallo' || estado.estado === 'ocupado' || Boolean(estado.mensaje));
</script>
<footer class="barra-estado" role="status" aria-live="polite"><span class:aviso><i class:girando={estado.estado === 'indexando'} aria-hidden="true"></i>{estado.estado === 'indexando' ? textos.indexando(estado.vistos, estado.total) : estado.estado === 'ocupado' ? textos.ocupado : estado.estado === 'fallo' ? textos.fallo : textos.alDia}{#if estado.mensaje}<span class="mensaje-estado"> · {estado.mensaje}</span>{/if}</span><span class="privacidad"><span aria-hidden="true">◇</span> {textos.local}</span></footer>
