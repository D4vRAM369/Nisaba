<script lang="ts">
  import { onMount } from 'svelte';
  import { textos } from '../lib/textos';
  import { llamar } from '../lib/api';
  let { propuesta = null, hecho }: { propuesta?: string | null; hecho: () => void } = $props();
  let lista = $state<string[]>([]);
  let proyectos = $state<[string, string][]>([]);
  let confirmar = $state<{ carpeta: string; n: number } | null>(null);
  let error = $state<string | null>(null);
  async function cargar() {
    try { [lista, proyectos] = await Promise.all([llamar<string[]>('exclusiones_lista'), llamar<[string, string][]>('proyectos_con_carpeta')]); }
    catch (e) { error = String(e); }
  }
  async function preparar(carpeta: string) {
    error = null;
    try {
      const n = await llamar<number>('contar_en_carpeta', { carpeta });
      if (n > 0) confirmar = { carpeta, n }; else await excluir(carpeta, false);
    } catch (e) { error = String(e); }
  }
  async function excluir(carpeta: string, borrar: boolean) {
    confirmar = null;
    try { await llamar('excluir', { carpeta, borrar }); await cargar(); hecho(); } catch (e) { error = String(e); }
  }
  async function quitar(carpeta: string) {
    try { await llamar('quitar_exclusion', { carpeta }); await cargar(); hecho(); } catch (e) { error = String(e); }
  }
  async function elegir() {
    try { const c = await llamar<string | null>('elegir_carpeta'); if (c) await preparar(c); } catch (e) { error = String(e); }
  }
  onMount(() => { void cargar().then(() => { if (propuesta) void preparar(propuesta); }); });
</script>
<section class="panel-salud panel-ajustes"><header><div><span class="etiqueta">{textos.ajustes}</span><h2>{textos.noIndexar}</h2><p>{textos.noIndexarDetalle}</p></div></header>
  {#if error}<p class="error-lector" role="alert">{error}</p>{/if}
  {#if confirmar}
    <div class="confirmacion" role="alertdialog" aria-labelledby="borrar-titulo">
      <h3 id="borrar-titulo">{textos.borrarGuardado(confirmar.carpeta, confirmar.n)}</h3>
      <p>{textos.borrarAviso}</p>
      <div class="acciones"><button class="boton-secundario" onclick={() => confirmar = null}>{textos.cancelar}</button><button class="boton-secundario" onclick={() => excluir(confirmar!.carpeta, false)}>{textos.borrarNo}</button><button class="boton-peligro" onclick={() => excluir(confirmar!.carpeta, true)}>{textos.borrarSi}</button></div>
    </div>
  {/if}
  {#if lista.length}<ul class="lista-exclusiones">{#each lista as c}<li><code>{c}</code><button class="boton-secundario" onclick={() => quitar(c)}>{textos.quitar}</button></li>{/each}</ul>
  {:else}<p class="sin-errores">{textos.sinExclusiones}</p>{/if}
  <div class="anadir-exclusion">
    <button class="boton-secundario" onclick={elegir}>{textos.anadirCarpeta}</button>
    <select aria-label={textos.elegirProyecto} onchange={e => { const v = e.currentTarget.value; e.currentTarget.value = ''; if (v) void preparar(v); }}>
      <option value="">{textos.elegirProyecto}</option>
      {#each proyectos.filter(([, c]) => !lista.includes(c)) as [nombre, carpeta]}<option value={carpeta}>{nombre || carpeta} — {carpeta}</option>{/each}
    </select>
  </div>
</section>
