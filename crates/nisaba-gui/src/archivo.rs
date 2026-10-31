use std::path::Path;
use std::sync::{Arc, Mutex};

use nisaba_core::archivo_web;
use nisaba_core::boveda::{self, LlaveMaestra, Nueva, BOVEDA, TABLILLA_WEB};
use nisaba_core::busqueda;
use nisaba_core::importar;
use nisaba_core::indexador::Rutas;
use nisaba_core::lector::{self, DetallesWeb, Lectura, Tramo, Version};
use nisaba_core::modelo::es_id_web;
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Default)]
pub struct Archivo {
    pub(crate) conn: Arc<Mutex<Option<Connection>>>,
    llave: Arc<Mutex<Option<LlaveMaestra>>>,
    nueva: Arc<Mutex<Option<Nueva>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoArchivo {
    NoExiste,
    Bloqueado,
    Desbloqueado,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumenImportacion {
    pub nuevas: usize,
    pub actualizadas: usize,
    pub versionadas: usize,
    pub iguales: usize,
    pub avisos: Vec<String>,
    pub ruta_origen: String,
}

impl Archivo {
    pub fn desbloqueado(&self) -> bool {
        self.conn.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn con_conexion<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        let g = self
            .conn
            .lock()
            .map_err(|_| "Estado interno bloqueado".to_string())?;
        match g.as_ref() {
            Some(c) => f(c).map(Some),
            None => Ok(None),
        }
    }

    pub fn abrir_conexion_propia(&self, home: &Path) -> Result<Connection, String> {
        let g = self
            .llave
            .lock()
            .map_err(|_| "Estado interno bloqueado".to_string())?;
        let Some(llave) = g.as_ref() else {
            return Err("El archivo web está bloqueado.".to_string());
        };
        archivo_web::abrir(home, llave).map_err(|e| e.to_string())
    }

    fn guardar_abierto(&self, conn: Connection, llave: LlaveMaestra) -> Result<(), String> {
        *self
            .conn
            .lock()
            .map_err(|_| "Estado interno bloqueado".to_string())? = Some(conn);
        *self
            .llave
            .lock()
            .map_err(|_| "Estado interno bloqueado".to_string())? = Some(llave);
        Ok(())
    }
}

pub fn estado_archivo_en(rutas: &Rutas, archivo: &Archivo) -> EstadoArchivo {
    if archivo.desbloqueado() {
        return EstadoArchivo::Desbloqueado;
    }
    if rutas.home.join(BOVEDA).exists() || rutas.home.join(TABLILLA_WEB).exists() {
        EstadoArchivo::Bloqueado
    } else {
        EstadoArchivo::NoExiste
    }
}

pub fn preparar_archivo_en(
    archivo: &Archivo,
    contrasena: &str,
    repeticion: &str,
) -> Result<String, String> {
    if contrasena != repeticion {
        return Err("Las contraseñas no coinciden.".to_string());
    }
    let nueva = boveda::preparar(contrasena).map_err(|e| e.to_string())?;
    let codigo = nueva.codigo().to_string();
    *archivo
        .nueva
        .lock()
        .map_err(|_| "Estado interno bloqueado".to_string())? = Some(nueva);
    Ok(codigo)
}

pub fn confirmar_archivo_en(
    rutas: &Rutas,
    archivo: &Archivo,
    confirmado: bool,
) -> Result<(), String> {
    if !confirmado {
        return Err("Confirma que has guardado el código de recuperación.".to_string());
    }
    let nueva = archivo
        .nueva
        .lock()
        .map_err(|_| "Estado interno bloqueado".to_string())?
        .take()
        .ok_or_else(|| "Primero prepara el archivo web.".to_string())?;
    std::fs::create_dir_all(&rutas.home).map_err(|e| e.to_string())?;
    let llave = nueva.guardar(&rutas.home).map_err(|e| e.to_string())?;
    let conn = archivo_web::abrir(&rutas.home, &llave).map_err(|e| e.to_string())?;
    archivo.guardar_abierto(conn, llave)
}

pub fn desbloquear_en(rutas: &Rutas, archivo: &Archivo, contrasena: &str) -> Result<(), String> {
    let llave = boveda::abrir(&rutas.home, contrasena).map_err(|e| e.to_string())?;
    let conn = archivo_web::abrir(&rutas.home, &llave).map_err(|e| e.to_string())?;
    archivo.guardar_abierto(conn, llave)
}

pub fn bloquear_en(archivo: &Archivo) {
    if let Ok(mut c) = archivo.conn.lock() {
        *c = None;
    }
    if let Ok(mut l) = archivo.llave.lock() {
        *l = None;
    }
    if let Ok(mut n) = archivo.nueva.lock() {
        *n = None;
    }
}

pub fn recuperar_archivo_en(
    rutas: &Rutas,
    archivo: &Archivo,
    codigo: &str,
    nueva_contrasena: &str,
    repeticion: &str,
) -> Result<(), String> {
    if nueva_contrasena != repeticion {
        return Err("Las contraseñas no coinciden.".to_string());
    }
    let llave =
        boveda::recuperar(&rutas.home, codigo, nueva_contrasena).map_err(|e| e.to_string())?;
    let conn = archivo_web::abrir(&rutas.home, &llave).map_err(|e| e.to_string())?;
    archivo.guardar_abierto(conn, llave)
}

pub fn cambiar_contrasena_en(
    rutas: &Rutas,
    actual: &str,
    nueva_contrasena: &str,
    repeticion: &str,
) -> Result<(), String> {
    if nueva_contrasena != repeticion {
        return Err("Las contraseñas no coinciden.".to_string());
    }
    boveda::cambiar_contrasena(&rutas.home, actual, nueva_contrasena).map_err(|e| e.to_string())
}

pub fn importar_zip_en(
    rutas: &Rutas,
    archivo: &Archivo,
    ruta: &str,
) -> Result<ResumenImportacion, String> {
    let path = Path::new(ruta);
    importar::reconocer(path)?;
    let mut conn = archivo.abrir_conexion_propia(&rutas.home)?;
    let r = importar::importar(&mut conn, path)?;
    Ok(ResumenImportacion {
        nuevas: r.nuevas,
        actualizadas: r.actualizadas,
        versionadas: r.versionadas,
        iguales: r.iguales,
        avisos: r.avisos,
        ruta_origen: ruta.to_string(),
    })
}

pub fn versiones_en(archivo: &Archivo, id: &str) -> Result<Vec<Version>, String> {
    if !es_id_web(id) {
        return Ok(vec![]);
    }
    let res = archivo.con_conexion(|conn| {
        let cid = busqueda::resolver_unico(conn, id)?;
        lector::versiones(conn, cid).map_err(|e| e.to_string())
    })?;
    res.ok_or_else(|| "El archivo web está bloqueado.".to_string())
}

pub fn leer_version_en(
    archivo: &Archivo,
    version_id: i64,
    tramo: Tramo,
) -> Result<Lectura, String> {
    let tramo = Tramo {
        cantidad: tramo.cantidad.clamp(1, 100),
        ..tramo
    };
    let res = archivo.con_conexion(|conn| {
        lector::leer_version(conn, version_id, &tramo).map_err(|e| e.to_string())
    })?;
    res.ok_or_else(|| "El archivo web está bloqueado.".to_string())
}

pub fn detalles_web_en(archivo: &Archivo, id: &str) -> Result<Option<DetallesWeb>, String> {
    if !es_id_web(id) {
        return Ok(None);
    }
    let res = archivo.con_conexion(|conn| {
        let cid = busqueda::resolver_unico(conn, id)?;
        lector::detalles_web(conn, cid).map_err(|e| e.to_string())
    })?;
    Ok(res)
}

pub fn leer_rama_en(
    archivo: &Archivo,
    id: &str,
    hoja: &str,
    tramo: Tramo,
) -> Result<Lectura, String> {
    let tramo = Tramo {
        cantidad: tramo.cantidad.clamp(1, 100),
        ..tramo
    };
    let res = archivo.con_conexion(|conn| {
        let cid = busqueda::resolver_unico(conn, id)?;
        lector::leer_rama(conn, cid, hoja, &tramo).map_err(|e| e.to_string())
    })?;
    res.ok_or_else(|| "El archivo web está bloqueado.".to_string())
}

#[tauri::command]
pub fn estado_archivo(archivo: State<'_, Archivo>) -> EstadoArchivo {
    estado_archivo_en(&Rutas::desde_entorno(), archivo.inner())
}

#[tauri::command]
pub async fn preparar_archivo(
    archivo: State<'_, Archivo>,
    contrasena: String,
    repeticion: String,
) -> Result<String, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        preparar_archivo_en(&archivo, &contrasena, &repeticion)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn confirmar_archivo(
    archivo: State<'_, Archivo>,
    confirmado: bool,
) -> Result<(), String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        confirmar_archivo_en(&Rutas::desde_entorno(), &archivo, confirmado)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn desbloquear(archivo: State<'_, Archivo>, contrasena: String) -> Result<(), String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        desbloquear_en(&Rutas::desde_entorno(), &archivo, &contrasena)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn bloquear(archivo: State<'_, Archivo>) {
    bloquear_en(archivo.inner());
}

#[tauri::command]
pub async fn recuperar_archivo(
    archivo: State<'_, Archivo>,
    codigo: String,
    nueva_contrasena: String,
    repeticion: String,
) -> Result<(), String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        recuperar_archivo_en(
            &Rutas::desde_entorno(),
            &archivo,
            &codigo,
            &nueva_contrasena,
            &repeticion,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cambiar_contrasena(
    actual: String,
    nueva_contrasena: String,
    repeticion: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        cambiar_contrasena_en(
            &Rutas::desde_entorno(),
            &actual,
            &nueva_contrasena,
            &repeticion,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn importar_zip(
    archivo: State<'_, Archivo>,
    ruta: String,
) -> Result<ResumenImportacion, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        importar_zip_en(&Rutas::desde_entorno(), &archivo, &ruta)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn elegir_zip(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(ruta) = app
            .dialog()
            .file()
            .add_filter("Exportación ZIP", &["zip"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let ruta = ruta.into_path().map_err(|e| e.to_string())?;
        Ok(Some(ruta.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn elegir_carpeta_importacion(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(ruta) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let ruta = ruta.into_path().map_err(|e| e.to_string())?;
        Ok(Some(ruta.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn versiones(
    archivo: State<'_, Archivo>,
    id_publico: String,
) -> Result<Vec<Version>, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || versiones_en(&archivo, &id_publico))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn leer_version(
    archivo: State<'_, Archivo>,
    version_id: i64,
    desde: Option<usize>,
    cantidad: Option<usize>,
    todo: Option<bool>,
) -> Result<Lectura, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        leer_version_en(
            &archivo,
            version_id,
            Tramo {
                desde,
                cantidad: cantidad.unwrap_or(20),
                alrededor_de: None,
                todo: todo.unwrap_or(false),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn detalles_web(
    archivo: State<'_, Archivo>,
    id_publico: String,
) -> Result<Option<DetallesWeb>, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || detalles_web_en(&archivo, &id_publico))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn leer_rama(
    archivo: State<'_, Archivo>,
    id_publico: String,
    hoja: String,
    desde: Option<usize>,
    cantidad: Option<usize>,
    todo: Option<bool>,
) -> Result<Lectura, String> {
    let archivo = archivo.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        leer_rama_en(
            &archivo,
            &id_publico,
            &hoja,
            Tramo {
                desde,
                cantidad: cantidad.unwrap_or(20),
                alrededor_de: None,
                todo: todo.unwrap_or(false),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
