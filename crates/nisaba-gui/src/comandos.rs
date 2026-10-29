use nisaba_core::{
    busqueda::{self, Filtros, Respuesta},
    indexador::Rutas,
    lector::{Lectura, Tramo},
    salud::FilaSalud,
};
use nisaba_core::{exclusiones, exportar};
use serde::{Deserialize, Serialize};
use tauri::Manager;
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

pub fn buscar_en(rutas: &Rutas, consulta: &str, f: FiltrosGui) -> Result<Respuesta, String> {
    let filtros = filtros_core(f)?;
    let conn =
        nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())?;
    busqueda::buscar_sin_contar(&conn, consulta, &filtros).map_err(|e| e.to_string())
}

pub fn contar_busqueda_en(rutas: &Rutas) -> Result<(), String> {
    let conn =
        nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())?;
    busqueda::contar_uso(&conn, "gui").map_err(|e| e.to_string())
}

pub fn recientes_en(
    rutas: &Rutas,
    f: FiltrosGui,
    desplazamiento: usize,
) -> Result<Respuesta, String> {
    let filtros = filtros_core(f)?;
    let conn =
        nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())?;
    busqueda::recientes_desde(&conn, &filtros, desplazamiento).map_err(|e| e.to_string())
}

pub fn leer_en(rutas: &Rutas, id: &str, tramo: Tramo) -> Result<Lectura, String> {
    let conn =
        nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())?;
    let cid = match busqueda::resolver(&conn, id).map_err(|e| e.to_string())? {
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
    nisaba_core::lector::leer(&conn, cid, &tramo).map_err(|e| e.to_string())
}

pub fn salud_en(rutas: &Rutas) -> Result<Vec<FilaSalud>, String> {
    let conn =
        nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())?;
    nisaba_core::salud::informe(&conn).map_err(|e| e.to_string())
}

pub fn proyectos_en(rutas: &Rutas) -> Result<Vec<String>, String> {
    let conn =
        nisaba_core::db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| e.to_string())?;
    let mut st = conn.prepare("SELECT DISTINCT proyecto FROM conversaciones_visibles WHERE proyecto IS NOT NULL AND proyecto <> '' ORDER BY proyecto COLLATE NOCASE").map_err(|e| e.to_string())?;
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

pub fn documento_en(rutas: &Rutas, id: &str, todo: bool) -> Result<Documento, String> {
    let conn = conectar(rutas)?;
    let markdown = exportar::documento(&conn, id, todo)?;
    Ok(Documento {
        secretos: exportar::posibles_secretos(&markdown),
        nombre_md: exportar::nombre_propuesto(&conn, id, "md")?,
        nombre_pdf: exportar::nombre_propuesto(&conn, id, "pdf")?,
        markdown,
    })
}

#[derive(Serialize)]
pub struct Origen {
    pub cwd: Option<String>,
    /// Comando para reanudar; `None` en subagentes, que no se reanudan solos.
    pub continuar: Option<String>,
}

pub fn origen_en(rutas: &Rutas, id: &str) -> Result<Origen, String> {
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
pub async fn documento(id_publico: String, todo: Option<bool>) -> Result<Documento, String> {
    en_segundo_plano(move || {
        documento_en(&Rutas::desde_entorno(), &id_publico, todo.unwrap_or(false))
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
pub async fn origen(id_publico: String) -> Result<Origen, String> {
    en_segundo_plano(move || origen_en(&Rutas::desde_entorno(), &id_publico)).await
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
pub async fn proyectos() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || proyectos_en(&Rutas::desde_entorno()))
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
pub async fn buscar(consulta: String, filtros: FiltrosGui) -> Result<Respuesta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        buscar_en(&Rutas::desde_entorno(), &consulta, filtros)
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
    filtros: FiltrosGui,
    desplazamiento: Option<usize>,
) -> Result<Respuesta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        recientes_en(
            &Rutas::desde_entorno(),
            filtros,
            desplazamiento.unwrap_or(0),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn leer(
    id_publico: String,
    desde: Option<usize>,
    cantidad: Option<usize>,
    alrededor_de: Option<i64>,
    todo: Option<bool>,
) -> Result<Lectura, String> {
    tauri::async_runtime::spawn_blocking(move || {
        leer_en(
            &Rutas::desde_entorno(),
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
