<script lang="ts">
  import { onMount } from 'svelte';
  import { textos } from './lib/textos';
  import { buscar, llamar, escuchar, escritorio, type Respuesta, type Filtros as Opciones, type Resultado, type EstadoIndexacion } from './lib/api';
  import Buscador from './componentes/Buscador.svelte';
  import Filtros from './componentes/Filtros.svelte';
  import ListaResultados from './componentes/ListaResultados.svelte';
  import BarraEstado from './componentes/BarraEstado.svelte';
  import Lector from './componentes/Lector.svelte';
  import Salud from './componentes/Salud.svelte';
  let consulta = $state('');
  let filtros = $state<Opciones>({ razonamiento: false, codigo: false, limite: 30 });
  let respuesta = $state<Respuesta>({ resultados: [], total_sesiones: 0, sugerencia: null });
  let seleccionado = $state<string | null>(null);
  let alrededorDe = $state<number | undefined>(undefined);
  let mostrarSalud = $state(false);
  let pendiente = $state(false);
  let error = $state<string | null>(null);
  let proyectos = $state<string[]>([]);
  let estado = $state<EstadoIndexacion>({ estado: 'indexando', vistos: 0, total: 0 });
  let peticion = 0;
  let temporizador: ReturnType<typeof setTimeout> | undefined;
  async function buscarAhora() {
    clearTimeout(temporizador);
    const actual = ++peticion;
    pendiente = true; error = null;
    try {
      const r = consulta.trim()
        ? await buscar(consulta.trim(), { ...filtros })
        : await llamar<Respuesta>('recientes', { filtros: { ...filtros } });
      if (actual === peticion) respuesta = r;
    }
    catch (e) { if (actual === peticion) { error = String(e); respuesta = { resultados: [], total_sesiones: 0, sugerencia: null }; } }
    finally { if (actual === peticion) pendiente = false; }
  }
  async function cargarMas() {
    if (pendiente || consulta.trim()) return;
    const actual = peticion;
    pendiente = true; error = null;
    try {
      const r = await llamar<Respuesta>('recientes', { filtros: { ...filtros }, desplazamiento: respuesta.resultados.length });
      if (actual === peticion) respuesta = { ...r, resultados: [...respuesta.resultados, ...r.resultados] };
    } catch (e) { if (actual === peticion) error = String(e); }
    finally { if (actual === peticion) pendiente = false; }
  }
  $effect(() => {
    consulta; filtros.fuente; filtros.proyecto; filtros.desde; filtros.razonamiento; filtros.codigo;
    ++peticion;
    temporizador = setTimeout(buscarAhora, 250);
    return () => clearTimeout(temporizador);
  });
  function abrir(r: Resultado) { seleccionado = r.id_publico; alrededorDe = consulta.trim() ? r.orden_mensaje : undefined; mostrarSalud = false; }
  function abrirId(id: string) { seleccionado = id; alrededorDe = undefined; mostrarSalud = false; }
  onMount(() => {
    let cerrado = false;
    const cancelar: (() => void)[] = [];
    if (!escritorio()) { estado = { estado: 'fallo', vistos: 0, total: 0, mensaje: textos.requiereApp }; return; }
    void (async () => {
      try {
        const off = await escuchar<EstadoIndexacion>('indexacion', e => { estado = e; if (e.estado === 'hecho') { void buscarAhora(); void llamar<string[]>('proyectos').then(p => { if (!cerrado) proyectos = p; }).catch(e => { if (!cerrado) error = String(e); }); } });
        if (cerrado) { off(); return; } cancelar.push(off);
        const abrirOff = await escuchar<string>('abrir-conversacion', abrirId);
        if (cerrado) { abrirOff(); return; } cancelar.push(abrirOff);
        const inicial = await llamar<string | null>('apertura_inicial');
        if (inicial) abrirId(inicial);
        estado = await llamar<EstadoIndexacion>('iniciar');
        proyectos = await llamar<string[]>('proyectos');
      } catch (e) { if (!cerrado) error = String(e); }
    })();
    return () => { cerrado = true; cancelar.forEach(off => off()); };
  });
</script>
<div class="app">
  <header class="cabecera-app"><div class="marca"><span class="glifos" aria-hidden="true">{textos.glifos}</span><div><h1>{textos.nombre}</h1><p>{textos.tablilla}</p></div></div><button class="boton-salud" class:activo={mostrarSalud} onclick={() => mostrarSalud = !mostrarSalud}>{mostrarSalud ? textos.volver : textos.salud} <span aria-hidden="true">◇</span></button></header>
  <main class="columnas"><aside class="explorador" aria-label={textos.buscar}><div class="busqueda-controles"><Buscador bind:consulta buscar={buscarAhora} /><Filtros bind:filtros {proyectos} /></div><ListaResultados {respuesta} {consulta} {pendiente} {seleccionado} {abrir} {cargarMas} /></aside>
    <section class="lector" aria-label={textos.selecciona}>{#if mostrarSalud}<Salud />{:else if seleccionado}{#key seleccionado}<Lector idPublico={seleccionado} {alrededorDe} abrir={abrirId} />{/key}{:else}<div class="lector-vacio"><span class="sello glifos" aria-hidden="true">{textos.glifos}</span><h2>{textos.lectorVacio}</h2><p>{textos.lectorVacioDetalle}</p><div class="linea-oro"></div><small>{textos.descripcion}</small></div>{/if}</section>
  </main>
  {#if error}<div class="error-global" role="alert">{error}</div>{/if}
  <BarraEstado {estado} />
</div>
