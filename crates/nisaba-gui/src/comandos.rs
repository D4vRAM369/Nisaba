use crate::archivo::Archivo;
use nisaba_core::{
    busqueda::{self, Filtros, Respuesta},
    indexador::Rutas,
    lector::{Lectura, Tramo},
    modelo::es_id_web,
    salud::FilaSalud,
};
use nisaba_core::{exclusiones, exportar};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct FiltrosGui {
    fuente: Option<String>,
    proyecto: Option<String>,
    desde: Option<String>,
    razonamiento: bool,
    codigo: bool,
    limite: Option<usize>,
}

fn filtros_core(f: FiltrosGui) -> Result<Filtros, String> {
    let fuente = f
        .fuente
        .as_deref()
        .map(|s| nisaba_core::modelo::Fuente::desde_str(s).ok_or("Fuente desconocida"))
        .transpose()?;
    Ok(Filtros {
        fuente,
        proyecto: f.proyecto,
        desde: f.desde,
        razonamiento: f.razonamiento,
        codigo: f.codigo,
        limite: f.limite.unwrap_or(30).clamp(1, 100),
    })
}

#[cfg(test)]
pub fn buscar_en(rutas: &Rutas, consulta: &str, f: FiltrosGui) -> Result<Respuesta, String> {
    buscar_con_archivo_en(rutas, &Archivo::default(), consulta, f)
}

pub fn buscar_con_archivo_en(
    rutas: &Rutas,
    archivo: &Archivo,
    consulta: &str,
    f: FiltrosGui,
) -> Result<Respuesta, String> {
    let filtros = filtros_core(f)?;
    let conn_local = conectar(rutas)?;
    if let Some(res_web) = archivo.con_conexion(|conn_web| {
        let r_loc = busqueda::buscar_sin_contar(&conn_local, consulta, &filtros)
            .map_err(|e| e.to_string())?;
        let r_web =
            busqueda::buscar_sin_contar(conn_web, consulta, &filtros).map_err(|e| e.to_string())?;
        Ok(busqueda::mezclar(r_loc, r_web, filtros.limite))
    })? {
        return Ok(res_web);
    }
    busqueda::buscar_sin_contar(&conn_local, consulta, &filtros).map_err(|e| e.to_string())
}

pub fn contar_busqueda_en(rutas: &Rutas) -> Result<(), String> {
    let conn = conectar(rutas)?;
    busqueda::contar_uso(&conn, "gui").map_err(|e| e.to_string())
}

#[cfg(test)]
pub fn recientes_en(
    rutas: &Rutas,
    f: FiltrosGui,
    desplazamiento: usize,
) -> Result<Respuesta, String> {
    recientes_con_archivo_en(rutas, &Archivo::default(), f, desplazamiento)
}

pub fn recientes_con_archivo_en(
    rutas: &Rutas,
    archivo: &Archivo,
    f: FiltrosGui,
    desplazamiento: usize,
) -> Result<Respuesta, String> {
    let filtros = filtros_core(f)?;
    let conn_local = conectar(rutas)?;
    if let Some(res_web) = archivo.con_conexion(|conn_web| {
        let tope = desplazamiento.saturating_add(filtros.limite);
        let f_amplio = Filtros {
            limite: tope,
            ..filtros.clone()
        };
        let resp_loc =
            busqueda::recientes_desde(&conn_local, &f_amplio, 0).map_err(|e| e.to_string())?;
        let resp_web =
            busqueda::recientes_desde(conn_web, &f_amplio, 0).map_err(|e| e.to_string())?;
        let total_sesiones = resp_loc
            .total_sesiones
            .saturating_add(resp_web.total_sesiones);
        let mut r_loc = resp_loc.resultados;
        r_loc.extend(resp_web.resultados);
        r_loc.sort_by(|a, b| {
            b.fin
                .as_deref()
                .unwrap_or("")
                .cmp(a.fin.as_deref().unwrap_or(""))
                .then_with(|| a.fuente.es_web().cmp(&b.fuente.es_web()))
                .then_with(|| a.id_publico.cmp(&b.id_publico))
        });
        let resultados = r_loc
            .into_iter()
            .skip(desplazamiento)
            .take(filtros.limite)
            .collect();
        Ok(Respuesta {
            resultados,
            sugerencia: None,
            total_sesiones,
        })
    })? {
        return Ok(res_web);
    }
    busqueda::recientes_desde(&conn_local, &filtros, desplazamiento).map_err(|e| e.to_string())
}

fn resolver_y_leer(conn: &rusqlite::Connection, id: &str, tramo: Tramo) -> Result<Lectura, String> {
    let cid = match busqueda::resolver(conn, id).map_err(|e| e.to_string())? {
        busqueda::Resolucion::Uno(c) => c,
        busqueda::Resolucion::Ninguno => return Err(format!("No encuentro la conversación {id}")),
        busqueda::Resolucion::Ambiguo(ids) => {
            return Err(format!("Id ambiguo: {id} encaja con {}", ids.join(", ")))
        }
        busqueda::Resolucion::Excluida => return Err(busqueda::EXCLUIDA.to_string()),
    };
    let tramo = Tramo {
        cantidad: tramo.cantidad.clamp(1, 100),
        ..tramo
    };
    nisaba_core::lector::leer(conn, cid, &tramo).map_err(|e| e.to_string())
}

#[cfg(test)]
pub fn leer_en(rutas: &Rutas, id: &str, tramo: Tramo) -> Result<Lectura, String> {
    leer_con_archivo_en(rutas, &Archivo::default(), id, tramo)
}

pub fn leer_con_archivo_en(
    rutas: &Rutas,
    archivo: &Archivo,
    id: &str,
    tramo: Tramo,
) -> Result<Lectura, String> {
    if es_id_web(id) {
        let res = archivo.con_conexion(|conn| resolver_y_leer(conn, id, tramo))?;
        return res.ok_or_else(|| "El archivo web está bloqueado.".to_string());
    }
    let conn = conectar(rutas)?;
    resolver_y_leer(&conn, id, tramo)
}

pub fn salud_en(rutas: &Rutas) -> Result<Vec<FilaSalud>, String> {
    let conn = conectar(rutas)?;
    nisaba_core::salud::informe(&conn).map_err(|e| e.to_string())
}

#[cfg(test)]
pub fn proyectos_en(rutas: &Rutas) -> Result<Vec<String>, String> {
    proyectos_con_archivo_en(rutas, &Archivo::default())
}

pub fn proyectos_con_archivo_en(rutas: &Rutas, archivo: &Archivo) -> Result<Vec<String>, String> {
    let conn = conectar(rutas)?;
    let mut lista = proyectos_de_conexion(&conn)?;
    if let Some(web_proys) = archivo.con_conexion(proyectos_de_conexion)? {
        for p in web_proys {
            if !lista.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
                lista.push(p);
            }
        }
        lista.sort_by_key(|a| a.to_lowercase());
    }
    Ok(lista)
}

fn proyectos_de_conexion(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    let mut st = conn
        .prepare(
            "SELECT DISTINCT proyecto FROM conversaciones_visibles
             WHERE proyecto IS NOT NULL AND proyecto <> ''
             ORDER BY proyecto COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let filas = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?;
    filas
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

fn conectar(rutas: &Rutas) -> Result<rusqlite::Connection, String> {
    nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct Documento {
    pub markdown: String,
    pub nombre_md: String,
    pub nombre_pdf: String,
    pub secretos: Vec<exportar::Hallazgo>,
}

fn documento_de_conexion(
    conn: &rusqlite::Connection,
    id: &str,
    todo: bool,
) -> Result<Documento, String> {
    let markdown = exportar::documento(conn, id, todo)?;
    Ok(Documento {
        secretos: exportar::posibles_secretos(&markdown),
        nombre_md: exportar::nombre_propuesto(conn, id, "md")?,
        nombre_pdf: exportar::nombre_propuesto(conn, id, "pdf")?,
        markdown,
    })
}

#[cfg(test)]
pub fn documento_en(rutas: &Rutas, id: &str, todo: bool) -> Result<Documento, String> {
    documento_con_archivo_en(rutas, &Archivo::default(), id, todo)
}

pub fn documento_con_archivo_en(
    rutas: &Rutas,
    archivo: &Archivo,
    id: &str,
    todo: bool,
) -> Result<Documento, String> {
    if es_id_web(id) {
        let res = archivo.con_conexion(|conn| documento_de_conexion(conn, id, todo))?;
        return res.ok_or_else(|| "El archivo web está bloqueado.".to_string());
    }
    let conn = conectar(rutas)?;
    documento_de_conexion(&conn, id, todo)
}

#[derive(Serialize)]
pub struct Origen {
    pub cwd: Option<String>,
    /// Comando para reanudar; `None` en subagentes y en conversaciones web.
    pub continuar: Option<String>,
}

pub fn origen_en(rutas: &Rutas, id: &str) -> Result<Origen, String> {
    origen_con_archivo_en(rutas, &Archivo::default(), id)
}

pub fn origen_con_archivo_en(rutas: &Rutas, archivo: &Archivo, id: &str) -> Result<Origen, String> {
    if es_id_web(id) {
        let _ = archivo;
        return Ok(Origen {
            cwd: None,
            continuar: None,
        });
    }
    let conn = conectar(rutas)?;
    let cid = busqueda::resolver_unico(&conn, id)?;
    let tramo = Tramo {
        cantidad: 1,
        ..Default::default()
    };
    let l = nisaba_core::lector::leer(&conn, cid, &tramo).map_err(|e| e.to_string())?;
    let continuar = if l.es_subagente {
        None
    } else {
        exportar::comando_continuar(l.fuente, &l.id_original, l.cwd.as_deref(), cfg!(windows))
    };
    Ok(Origen {
        cwd: l.cwd,
        continuar,
    })
}

/// Fuera de Flatpak se avisa si la carpeta ya no está; dentro, el sandbox no la ve
/// y decide el portal.
pub fn comprobar_carpeta(ruta: &str, flatpak: bool) -> Result<(), String> {
    if flatpak || std::path::Path::new(ruta).is_dir() {
        Ok(())
    } else {
        Err(format!("Esta carpeta ya no existe: {ruta}"))
    }
}

/// La carpeta se resuelve aquí a partir de la conversación: la ventana no elige rutas.
pub fn carpeta_para_abrir_en(rutas: &Rutas, id: &str, flatpak: bool) -> Result<String, String> {
    let cwd = origen_en(rutas, id)?
        .cwd
        .ok_or("Esta conversación no guarda su carpeta")?;
    comprobar_carpeta(&cwd, flatpak)?;
    Ok(cwd)
}

pub fn exclusiones_lista_en(rutas: &Rutas) -> Result<Vec<String>, String> {
    exclusiones::lista(&conectar(rutas)?).map_err(|e| e.to_string())
}

pub fn contar_en_carpeta_en(rutas: &Rutas, carpeta: &str) -> Result<usize, String> {
    exclusiones::contar(&conectar(rutas)?, carpeta).map_err(|e| e.to_string())
}

pub fn excluir_en(rutas: &Rutas, carpeta: &str, borrar: bool) -> Result<usize, String> {
    exclusiones::anadir(&conectar(rutas)?, rutas, carpeta, borrar).map_err(|e| e.to_string())
}

pub fn quitar_exclusion_en(rutas: &Rutas, carpeta: &str) -> Result<(), String> {
    exclusiones::quitar(&conectar(rutas)?, carpeta).map_err(|e| e.to_string())
}

/// (proyecto, carpeta) de las sesiones madre visibles.
pub fn proyectos_con_carpeta_en(rutas: &Rutas) -> Result<Vec<(String, String)>, String> {
    let conn = conectar(rutas)?;
    let mut st = conn
        .prepare(
            "SELECT DISTINCT COALESCE(proyecto, ''), cwd FROM conversaciones_visibles
             WHERE cwd IS NOT NULL AND padre_id IS NULL
             ORDER BY proyecto COLLATE NOCASE, cwd",
        )
        .map_err(|e| e.to_string())?;
    let filas = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?;
    filas
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

/// Ejecuta `f` fuera del hilo de la ventana.
async fn en_segundo_plano<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn documento(
    archivo: State<'_, Archivo>,
    id_publico: String,
    todo: Option<bool>,
) -> Result<Documento, String> {
    let archivo = archivo.inner().clone();
    en_segundo_plano(move || {
        documento_con_archivo_en(
            &Rutas::desde_entorno(),
            &archivo,
            &id_publico,
            todo.unwrap_or(false),
        )
    })
    .await
}

/// Pide la ruta con el diálogo nativo y guarda ahí. La ventana nunca elige la ruta:
/// solo propone el nombre. `None` si se cancela.
#[tauri::command]
pub async fn guardar_como(
    app: tauri::AppHandle,
    nombre: String,
    contenido: String,
) -> Result<Option<String>, String> {
    en_segundo_plano(move || {
        let Some(ruta) = app
            .dialog()
            .file()
            .set_file_name(&nombre)
            .add_filter("Markdown", &["md"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let ruta = ruta.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&ruta, contenido).map_err(|e| format!("{}: {e}", ruta.display()))?;
        Ok(Some(ruta.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
pub async fn origen(archivo: State<'_, Archivo>, id_publico: String) -> Result<Origen, String> {
    let archivo = archivo.inner().clone();
    en_segundo_plano(move || origen_con_archivo_en(&Rutas::desde_entorno(), &archivo, &id_publico))
        .await
}

#[tauri::command]
pub async fn abrir_carpeta(app: tauri::AppHandle, id_publico: String) -> Result<(), String> {
    en_segundo_plano(move || {
        let ruta = carpeta_para_abrir_en(
            &Rutas::desde_entorno(),
            &id_publico,
            std::env::var_os("FLATPAK_ID").is_some(),
        )?;
        app.opener()
            .open_path(&ruta, None::<&str>)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn exclusiones_lista() -> Result<Vec<String>, String> {
    en_segundo_plano(move || exclusiones_lista_en(&Rutas::desde_entorno())).await
}

#[tauri::command]
pub async fn contar_en_carpeta(carpeta: String) -> Result<usize, String> {
    en_segundo_plano(move || contar_en_carpeta_en(&Rutas::desde_entorno(), &carpeta)).await
}

#[tauri::command]
pub async fn excluir(
    app: tauri::AppHandle,
    carpeta: String,
    borrar: bool,
) -> Result<usize, String> {
    let n = en_segundo_plano(move || excluir_en(&Rutas::desde_entorno(), &carpeta, borrar)).await?;
    app.state::<crate::indexacion::Indexacion>()
        .reindexar(app.clone());
    Ok(n)
}

#[tauri::command]
pub async fn quitar_exclusion(app: tauri::AppHandle, carpeta: String) -> Result<(), String> {
    en_segundo_plano(move || quitar_exclusion_en(&Rutas::desde_entorno(), &carpeta)).await?;
    app.state::<crate::indexacion::Indexacion>()
        .reindexar(app.clone());
    Ok(())
}

#[tauri::command]
pub async fn proyectos_con_carpeta() -> Result<Vec<(String, String)>, String> {
    en_segundo_plano(move || proyectos_con_carpeta_en(&Rutas::desde_entorno())).await
}

#[tauri::command]
pub async fn elegir_carpeta(app: tauri::AppHandle) -> Result<Option<String>, String> {
    en_segundo_plano(move || {
        let Some(ruta) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let ruta = ruta.into_path().map_err(|e| e.to_string())?;
        Ok(Some(ruta.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
pub async fn proyectos(archivo: State<'_, Archivo>) -> Result<Vec<String>, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        proyectos_con_archivo_en(&Rutas::desde_entorno(), &archivo)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn aviso_limpieza() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = nisaba_core::ajustes_claude();
        nisaba_core::salud::aviso_limpieza_claude(&settings)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn buscar(
    archivo: State<'_, Archivo>,
    consulta: String,
    filtros: FiltrosGui,
) -> Result<Respuesta, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        buscar_con_archivo_en(&Rutas::desde_entorno(), &archivo, &consulta, filtros)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn contar_busqueda() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || contar_busqueda_en(&Rutas::desde_entorno()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn recientes(
    archivo: State<'_, Archivo>,
    filtros: FiltrosGui,
    desplazamiento: Option<usize>,
) -> Result<Respuesta, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        recientes_con_archivo_en(
            &Rutas::desde_entorno(),
            &archivo,
            filtros,
            desplazamiento.unwrap_or(0),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn leer(
    archivo: State<'_, Archivo>,
    id_publico: String,
    desde: Option<usize>,
    cantidad: Option<usize>,
    alrededor_de: Option<i64>,
    todo: Option<bool>,
) -> Result<Lectura, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        leer_con_archivo_en(
            &Rutas::desde_entorno(),
            &archivo,
            &id_publico,
            Tramo {
                desde,
                cantidad: cantidad.unwrap_or(20),
                alrededor_de,
                todo: todo.unwrap_or(false),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn salud() -> Result<Vec<FilaSalud>, String> {
    tauri::async_runtime::spawn_blocking(move || salud_en(&Rutas::desde_entorno()))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archivo::{
        bloquear_en, confirmar_archivo_en, desbloquear_en, estado_archivo_en, preparar_archivo_en,
        EstadoArchivo,
    };
    use nisaba_core::{
        db,
        indexador::{indexar, ResultadoIndexado},
    };
    fn preparar() -> (tempfile::TempDir, Rutas) {
        let d = tempfile::tempdir().unwrap();
        let rutas = Rutas {
            home: d.path().join("home"),
            claude: d.path().join("claude"),
            codex: d.path().join("codex"),
        };
        let origen = rutas.codex.join("sessions");
        std::fs::create_dir_all(&origen).unwrap();
        std::fs::write(origen.join("rollout-demo.jsonl"), concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"demo-codex\",\"cwd\":\"/tmp/Demo\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Quiero subtitulos para mi pelicula\"}]}}\n"
        )).unwrap();
        let mut c = db::abrir(&rutas.home.join("tablilla.db")).unwrap();
        assert!(matches!(
            indexar(&mut c, &rutas, &mut |_| {}),
            ResultadoIndexado::Hecho { errores: 0, .. }
        ));
        (d, rutas)
    }
    /// Copia los fixtures de Claude Code del núcleo (sesión con subagente) e indexa.
    fn preparar_claude() -> (tempfile::TempDir, Rutas) {
        let d = tempfile::tempdir().unwrap();
        let rutas = Rutas {
            home: d.path().join("home"),
            claude: d.path().join("claude"),
            codex: d.path().join("codex"),
        };
        let fx = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../nisaba-core/tests/fixtures/claude/-home-u-Proyectos-Demo");
        copiar(&fx, &rutas.claude.join("-home-u-Proyectos-Demo"));
        let llaves = rutas.claude.join("-home-u-Llaves");
        std::fs::create_dir_all(&llaves).unwrap();
        std::fs::write(
            llaves.join("5ec0e700-0000-4000-8000-000000000003.jsonl"),
            "{\"type\":\"user\",\"uuid\":\"s1\",\"parentUuid\":null,\"sessionId\":\"5ec0e700-0000-4000-8000-000000000003\",\"cwd\":\"/home/u/Llaves\",\"timestamp\":\"2026-10-02T10:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"usa sk-ant-api03-AbCdEfGhIjKlMnOpQra3F9\"}}\n",
        )
        .unwrap();
        let mut c = db::abrir(&rutas.home.join("tablilla.db")).unwrap();
        indexar(&mut c, &rutas, &mut |_| {});
        (d, rutas)
    }

    fn copiar(origen: &std::path::Path, destino: &std::path::Path) {
        std::fs::create_dir_all(destino).unwrap();
        for e in std::fs::read_dir(origen).unwrap() {
            let e = e.unwrap();
            let d = destino.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copiar(&e.path(), &d);
            } else {
                std::fs::copy(e.path(), d).unwrap();
            }
        }
    }

    fn sembrar_web(rutas: &Rutas, archivo: &Archivo, contrasena: &str) {
        let _codigo = preparar_archivo_en(archivo, contrasena, contrasena).unwrap();
        confirmar_archivo_en(rutas, archivo, true).unwrap();
        archivo
            .con_conexion(|conn| {
                conn.execute(
                    "INSERT INTO conversaciones(id, fuente, id_original, titulo, fin)
                     VALUES (1, 'claude-web', 'web00001abc', 'Charla web', '2026-10-30T10:00:00Z')",
                    [],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO mensajes(id, conversacion_id, orden, rol, oculto)
                     VALUES (1, 1, 0, 'usuario', 0)",
                    [],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO partes(id, mensaje_id, orden, tipo, contenido)
                     VALUES (1, 1, 0, 'texto', 'Quiero subtitulos desde claude web')",
                    [],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn bloqueado_solo_devuelve_local() {
        let (_d, rutas) = preparar();
        let archivo = Archivo::default();
        sembrar_web(&rutas, &archivo, "clave suficientemente larga");
        bloquear_en(&archivo);
        assert_eq!(
            estado_archivo_en(&rutas, &archivo),
            EstadoArchivo::Bloqueado
        );

        let r =
            buscar_con_archivo_en(&rutas, &archivo, "subtitulos", FiltrosGui::default()).unwrap();
        assert_eq!(r.resultados.len(), 1);
        assert_eq!(r.resultados[0].id_publico, "cx-mo-codex");
        assert!(leer_con_archivo_en(&rutas, &archivo, "cw-web00001", Tramo::default()).is_err());
    }

    #[test]
    fn desbloqueado_mezcla() {
        let (_d, rutas) = preparar();
        let archivo = Archivo::default();
        let clave = "clave suficientemente larga";
        sembrar_web(&rutas, &archivo, clave);
        bloquear_en(&archivo);
        desbloquear_en(&rutas, &archivo, clave).unwrap();
        assert_eq!(
            estado_archivo_en(&rutas, &archivo),
            EstadoArchivo::Desbloqueado
        );

        let r =
            buscar_con_archivo_en(&rutas, &archivo, "subtitulos", FiltrosGui::default()).unwrap();
        assert_eq!(r.resultados.len(), 2);
        assert!(r.resultados.iter().any(|x| x.id_publico == "cx-mo-codex"));
        assert!(r.resultados.iter().any(|x| x.id_publico == "cw-web00001"));

        let lec = leer_con_archivo_en(&rutas, &archivo, "cw-web00001", Tramo::default()).unwrap();
        assert_eq!(lec.titulo, "Charla web");
    }

    #[test]
    fn bloquear_suelta_la_conexion() {
        let (_d, rutas) = preparar();
        let archivo = Archivo::default();
        sembrar_web(&rutas, &archivo, "clave suficientemente larga");
        assert!(archivo.desbloqueado());

        bloquear_en(&archivo);
        assert!(!archivo.desbloqueado());
        assert!(archivo.conn.lock().unwrap().is_none());
        assert!(archivo.abrir_conexion_propia(&rutas.home).is_err());
    }

    #[test]
    fn confirmar_sin_preparar_falla() {
        let (_d, rutas) = preparar();
        let archivo = Archivo::default();
        let err = confirmar_archivo_en(&rutas, &archivo, true).unwrap_err();
        assert!(err.contains("prepara"), "{err}");
        assert_eq!(estado_archivo_en(&rutas, &archivo), EstadoArchivo::NoExiste);
    }

    #[test]
    fn origen_web_sin_continuar_ni_carpeta() {
        let (_d, rutas) = preparar();
        let archivo = Archivo::default();
        sembrar_web(&rutas, &archivo, "clave suficientemente larga");
        let orig = origen_con_archivo_en(&rutas, &archivo, "cw-web00001").unwrap();
        assert_eq!(orig.cwd, None);
        assert_eq!(orig.continuar, None);
    }

    #[test]
    fn documento_avisa_de_secretos() {
        let (_d, rutas) = preparar_claude();
        let doc = documento_en(&rutas, "cc-5ec0e700", false).unwrap();
        assert_eq!(doc.secretos.len(), 1);
        assert_eq!(doc.secretos[0].tipo, "Anthropic");
        assert!(doc.nombre_md.ends_with(".md") && doc.nombre_pdf.ends_with(".pdf"));
        assert!(doc.markdown.contains("/home/u/Llaves"));
        let limpio = documento_en(&rutas, "cc-a1b2c3d4", false).unwrap();
        assert!(limpio.secretos.is_empty());
    }

    #[test]
    fn origen_de_subagente_sin_continuar() {
        let (_d, rutas) = preparar_claude();
        let madre = origen_en(&rutas, "cc-a1b2c3d4").unwrap();
        assert_eq!(madre.cwd.as_deref(), Some("/home/u/Proyectos/Demo"));
        assert!(madre
            .continuar
            .unwrap()
            .ends_with("claude --resume a1b2c3d4-0000-4000-8000-000000000001"));
        let hijo = origen_en(&rutas, "cc-agent-zz").unwrap();
        assert!(hijo.cwd.is_some());
        assert_eq!(hijo.continuar, None);
    }

    #[test]
    fn excluir_y_quitar_desde_gui() {
        let (_d, rutas) = preparar_claude();
        let demo = "/home/u/Proyectos/Demo";
        assert!(proyectos_con_carpeta_en(&rutas)
            .unwrap()
            .contains(&("Demo".to_string(), demo.to_string())));
        assert_eq!(contar_en_carpeta_en(&rutas, demo).unwrap(), 2);
        assert_eq!(excluir_en(&rutas, demo, false).unwrap(), 2);
        assert_eq!(exclusiones_lista_en(&rutas).unwrap(), [demo]);
        assert_eq!(
            leer_en(&rutas, "cc-a1b2c3d4", Tramo::default()).unwrap_err(),
            busqueda::EXCLUIDA
        );
        quitar_exclusion_en(&rutas, demo).unwrap();
        assert!(exclusiones_lista_en(&rutas).unwrap().is_empty());
        assert!(leer_en(&rutas, "cc-a1b2c3d4", Tramo::default()).is_ok());
    }

    #[test]
    fn la_carpeta_a_abrir_sale_de_la_conversacion() {
        let (_d, rutas) = preparar_claude();
        // El cwd del fixture no existe en este equipo.
        assert_eq!(
            carpeta_para_abrir_en(&rutas, "cc-a1b2c3d4", false).unwrap_err(),
            "Esta carpeta ya no existe: /home/u/Proyectos/Demo"
        );
        assert_eq!(
            carpeta_para_abrir_en(&rutas, "cc-a1b2c3d4", true).unwrap(),
            "/home/u/Proyectos/Demo"
        );
        assert!(carpeta_para_abrir_en(&rutas, "cc-ffffffff", true).is_err());
    }

    #[test]
    fn abrir_carpeta_inexistente_avisa() {
        let d = tempfile::tempdir().unwrap();
        let falta = d.path().join("no-existe");
        let ruta = falta.to_string_lossy();
        assert_eq!(
            comprobar_carpeta(&ruta, false).unwrap_err(),
            format!("Esta carpeta ya no existe: {ruta}")
        );
        // En Flatpak el sandbox no la ve: se deja decidir al portal.
        assert!(comprobar_carpeta(&ruta, true).is_ok());
        assert!(comprobar_carpeta(&d.path().to_string_lossy(), false).is_ok());
    }

    #[test]
    fn buscar_al_escribir_no_cuenta_y_abrir_un_resultado_si() {
        let (_d, rutas) = preparar();
        let uso = || -> i64 {
            db::abrir(&rutas.home.join("tablilla.db"))
                .unwrap()
                .query_row(
                    "SELECT coalesce(sum(busquedas),0) FROM uso WHERE canal='gui'",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };
        for _ in 0..3 {
            let r = buscar_en(&rutas, "subtitulos", FiltrosGui::default()).unwrap();
            assert_eq!(r.resultados.len(), 1);
        }
        assert_eq!(uso(), 0);
        contar_busqueda_en(&rutas).unwrap();
        assert_eq!(uso(), 1);
    }
    #[test]
    fn leer_id_desconocido_es_error() {
        let (_d, rutas) = preparar();
        assert!(leer_en(&rutas, "cx-inexistente", Tramo::default())
            .unwrap_err()
            .contains("No encuentro"));
    }
    #[test]
    fn salud_una_fila_por_fuente() {
        let (_d, rutas) = preparar();
        let filas = salud_en(&rutas).unwrap();
        assert_eq!(filas.len(), 2);
        assert_eq!(filas[1].parseados, 1);
    }
    #[test]
    fn proyectos_devuelve_nombres_del_indice() {
        let (_d, rutas) = preparar();
        assert_eq!(proyectos_en(&rutas).unwrap(), vec!["Demo"]);
    }
    #[test]
    fn recientes_muestra_sesiones_sin_contar_una_busqueda() {
        let (_d, rutas) = preparar();
        let r = recientes_en(&rutas, FiltrosGui::default(), 0).unwrap();
        assert_eq!(r.resultados.len(), 1);
        let conn = db::abrir(&rutas.home.join("tablilla.db")).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM uso", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn recientes_y_buscar_rechazan_la_misma_fuente_desconocida() {
        let (_d, rutas) = preparar();
        let f = || FiltrosGui {
            fuente: Some("desconocida".into()),
            ..Default::default()
        };
        assert_eq!(
            recientes_en(&rutas, f(), 0).unwrap_err(),
            "Fuente desconocida"
        );
        assert_eq!(
            buscar_en(&rutas, "subtitulos", f()).unwrap_err(),
            "Fuente desconocida"
        );
    }
}
