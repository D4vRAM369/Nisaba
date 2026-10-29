use nisaba_core::{
    busqueda::{self, Filtros, Respuesta},
    indexador::Rutas,
    lector::{Lectura, Tramo},
    salud::FilaSalud,
};
use serde::Deserialize;

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
    let mut st = conn.prepare("SELECT DISTINCT proyecto FROM conversaciones WHERE proyecto IS NOT NULL AND proyecto <> '' ORDER BY proyecto COLLATE NOCASE").map_err(|e| e.to_string())?;
    let filas = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?;
    filas
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
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
        let settings = std::env::var_os("NISABA_CLAUDE_SETTINGS")
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".claude/settings.json")
            });
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
