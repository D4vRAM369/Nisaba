<script lang="ts">
  import { textos } from '../lib/textos';
  import { llamar, esIdWeb, type Documento } from '../lib/api';
  import { renderizar } from '../lib/markdown';
  let { idPublico, todo }: { idPublico: string; todo: boolean } = $props();
  let pendiente = $state<{ doc: Documento; formato: 'md' | 'pdf' } | null>(null);
  let aviso = $state<string | null>(null);
  let ocupado = $state(false);
  async function pedir(formato: 'md' | 'pdf') {
    ocupado = true; aviso = null;
    try {
      const doc = await llamar<Documento>('documento', { idPublico, todo });
      if (doc.secretos.length) pendiente = { doc, formato }; else await sacar(doc, formato);
    } catch (e) { aviso = String(e); }
    finally { ocupado = false; }
  }
  async function sacar(doc: Documento, formato: 'md' | 'pdf') {
    pendiente = null;
    if (formato === 'pdf') {
      if (esIdWeb(idPublico)) aviso = textos.avisoExportarWeb;
      return imprimir(doc);
    }
    const ruta = await llamar<string | null>('guardar_como', { nombre: doc.nombre_md, contenido: doc.markdown });
    if (ruta) aviso = esIdWeb(idPublico) ? textos.guardadoSinCifrar(ruta) : textos.guardadoEn(ruta);
  }
  // El HTML sale saneado por `renderizar`; la hoja @media print solo deja ver #impresion.
  function imprimir(doc: Documento) {
    const hoja = Object.assign(document.createElement('div'), { id: 'impresion', innerHTML: renderizar(doc.markdown) });
    document.body.append(hoja);
    const titulo = document.title;
    document.title = doc.nombre_pdf.replace(/\.pdf$/, '');
    const limpiar = () => { hoja.remove(); document.title = titulo; };
    window.addEventListener('afterprint', limpiar, { once: true });
    window.print();
  }
</script>
<div class="exportar">
  <button class="boton-exportar" disabled={ocupado} title={textos.exportarMdTitulo} onclick={() => pedir('md')}>{textos.exportarMd}</button>
  <button class="boton-exportar" disabled={ocupado} title={textos.exportarPdfTitulo} onclick={() => pedir('pdf')}>{textos.exportarPdf}</button>
</div>
{#if aviso}<p class="aviso-exportar" role="status">{aviso}</p>{/if}
{#if pendiente}
  <div class="confirmacion" role="alertdialog" aria-labelledby="secretos-titulo">
    <h3 id="secretos-titulo">{textos.secretosTitulo(pendiente.doc.secretos.length)}</h3>
    <p>{textos.secretosDetalle}{#if esIdWeb(idPublico)} {textos.avisoExportarWeb}{/if}</p>
    <ul>{#each pendiente.doc.secretos as h}<li><strong>{h.tipo}</strong> <code>{h.muestra}</code></li>{/each}</ul>
    <div class="acciones"><button class="boton-secundario" onclick={() => pendiente = null}>{textos.cancelar}</button><button class="boton-peligro" onclick={() => sacar(pendiente!.doc, pendiente!.formato).catch(e => aviso = String(e))}>{textos.exportarIgualmente}</button></div>
  </div>
{/if}
