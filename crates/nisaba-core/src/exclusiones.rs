//! Carpetas que el usuario no quiere indexar. Se comparan por componentes tras una
//! normalización léxica: nunca contra el disco, porque la carpeta puede no existir ya.

use crate::indexador::{destino_copia, Rutas};
use crate::modelo::Fuente;
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::Path;

const WINDOWS: bool = cfg!(windows);

/// Componentes de la ruta: `/` y `\` equivalen; sin vacíos, `.` ni `..`; en minúsculas en Windows.
pub fn normalizar(ruta: &str, windows: bool) -> Vec<String> {
    let mut partes: Vec<String> = Vec::new();
    for p in ruta.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                partes.pop();
            }
            _ if windows => partes.push(p.to_lowercase()),
            _ => partes.push(p.to_string()),
        }
    }
    partes
}

/// `cwd` es `excluida` o una de sus subcarpetas.
pub fn cubre(excluida: &str, cwd: &str, windows: bool) -> bool {
    normalizar(cwd, windows).starts_with(&normalizar(excluida, windows))
}

pub(crate) fn cubierta(lista: &[String], cwd: &str) -> bool {
    lista.iter().any(|e| cubre(e, cwd, WINDOWS))
}

pub fn lista(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut st = conn.prepare("SELECT ruta FROM exclusiones ORDER BY ruta")?;
    let filas = st.query_map([], |f| f.get(0))?;
    filas.collect()
}

fn error_io(e: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

/// Recalcula `conversaciones.excluida` con la lista actual.
pub(crate) fn marcar_conversaciones(conn: &Connection, lista: &[String]) -> rusqlite::Result<()> {
    let filas: Vec<(i64, Option<String>)> = conn
        .prepare("SELECT id, cwd FROM conversaciones")?
        .query_map([], |f| Ok((f.get(0)?, f.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut st =
        conn.prepare("UPDATE conversaciones SET excluida = ?1 WHERE id = ?2 AND excluida <> ?1")?;
    for (id, cwd) in filas {
        let excluida = cwd.is_some_and(|c| cubierta(lista, &c));
        st.execute(params![excluida as i64, id])?;
    }
    Ok(())
}

/// Conversaciones ya guardadas bajo `carpeta`, con sus subagentes.
fn afectadas(conn: &Connection, carpeta: &str) -> rusqlite::Result<HashSet<i64>> {
    let solo = [carpeta.to_string()];
    let todas: Vec<(i64, Option<i64>, Option<String>)> = conn
        .prepare("SELECT id, padre_id, cwd FROM conversaciones")?
        .query_map([], |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let madres: HashSet<i64> = todas
        .iter()
        .filter(|(_, _, cwd)| cwd.as_deref().is_some_and(|c| cubierta(&solo, c)))
        .map(|(id, _, _)| *id)
        .collect();
    let mut ids = madres.clone();
    ids.extend(
        todas
            .iter()
            .filter(|(_, p, _)| p.is_some_and(|p| madres.contains(&p)))
            .map(|(id, _, _)| *id),
    );
    Ok(ids)
}

/// Cuántas conversaciones guardadas afectaría excluir `carpeta` (para preguntar antes).
pub fn contar(conn: &Connection, carpeta: &str) -> rusqlite::Result<usize> {
    Ok(afectadas(conn, carpeta)?.len())
}

/// Excluye `carpeta`. Devuelve cuántas conversaciones ya indexadas afecta (con sus subagentes).
/// Con `borrar`, las elimina de la tablilla junto con sus copias: la única excepción
/// a "la tablilla solo crece", y quien llama la ha confirmado antes.
pub fn anadir(
    conn: &Connection,
    rutas: &Rutas,
    carpeta: &str,
    borrar: bool,
) -> rusqlite::Result<usize> {
    // Una carpeta sin componentes ("", "/", ".") lo cubriría todo.
    if normalizar(carpeta, WINDOWS).is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "carpeta no válida para excluir: {carpeta:?}"
        )));
    }
    // Espera a que termine cualquier indexación: si no, una pasada que empezó con la
    // lista anterior podría volver a guardar lo que se excluye (o se borra) ahora.
    let cerrojo = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(rutas.home.join("index.lock"))
        .map_err(error_io)?;
    cerrojo.lock().map_err(error_io)?;
    conn.execute(
        "INSERT OR IGNORE INTO exclusiones(ruta, desde)
         VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [carpeta],
    )?;
    marcar_conversaciones(conn, &lista(conn)?)?;
    let solo = [carpeta.to_string()];
    let ids = afectadas(conn, carpeta)?;

    // Archivos afectados: por su cwd o por ser la fuente de una conversación afectada.
    let mut archivos: Vec<(String, String, Option<String>)> = Vec::new();
    {
        let mut st = conn.prepare(
            "SELECT a.ruta, a.fuente, COALESCE(a.cwd, c.cwd), c.id
             FROM archivos a LEFT JOIN conversaciones c ON c.ruta = a.ruta",
        )?;
        let filas = st.query_map([], |f| {
            Ok((
                f.get::<_, String>(0)?,
                f.get::<_, String>(1)?,
                f.get::<_, Option<String>>(2)?,
                f.get::<_, Option<i64>>(3)?,
            ))
        })?;
        for fila in filas {
            let (ruta, fuente, cwd, conv) = fila?;
            let afectado = cwd.as_deref().is_some_and(|c| cubierta(&solo, c))
                || conv.is_some_and(|c| ids.contains(&c));
            if afectado {
                archivos.push((ruta, fuente, cwd));
            }
        }
    }
    for (ruta, _, cwd) in &archivos {
        conn.execute(
            "UPDATE archivos SET estado = 'excluida', error = NULL, cwd = ?1 WHERE ruta = ?2",
            params![cwd, ruta],
        )?;
    }

    if borrar {
        if !ids.is_empty() {
            let lista_ids = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            conn.execute(
                &format!("DELETE FROM conversaciones WHERE id IN ({lista_ids})"),
                [],
            )?;
        }
        for (ruta, fuente, _) in &archivos {
            if let Some(fuente) = Fuente::desde_str(fuente) {
                borrar_copias(rutas, fuente, Path::new(ruta)).map_err(error_io)?;
            }
        }
    }
    Ok(ids.len())
}

/// Borra la copia de una sesión y sus versiones `.encogido-*`.
fn borrar_copias(rutas: &Rutas, fuente: Fuente, origen: &Path) -> std::io::Result<()> {
    let Ok(copia) = destino_copia(rutas, fuente, origen) else {
        return Ok(());
    };
    let (Some(dir), Some(nombre)) = (copia.parent(), copia.file_name()) else {
        return Ok(());
    };
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let nombre = nombre.to_string_lossy().to_string();
    let encogido = format!("{nombre}.encogido-");
    for e in entradas {
        let e = e?;
        let n = e.file_name().to_string_lossy().to_string();
        if n == nombre || n.starts_with(&encogido) {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

/// Quita la exclusión. Lo excluido sin borrar vuelve a verse; los archivos que ya no
/// cubre ninguna exclusión quedan pendientes para la siguiente pasada.
pub fn quitar(conn: &Connection, carpeta: &str) -> rusqlite::Result<()> {
    let objetivo = normalizar(carpeta, WINDOWS);
    let guardadas = lista(conn)?;
    for g in guardadas
        .iter()
        .filter(|g| normalizar(g, WINDOWS) == objetivo)
    {
        conn.execute("DELETE FROM exclusiones WHERE ruta = ?1", [g])?;
    }
    let quedan = lista(conn)?;
    marcar_conversaciones(conn, &quedan)?;
    let excluidos: Vec<(String, Option<String>)> = conn
        .prepare("SELECT ruta, cwd FROM archivos WHERE estado = 'excluida'")?
        .query_map([], |f| Ok((f.get(0)?, f.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (ruta, cwd) in excluidos {
        if !cwd.is_some_and(|c| cubierta(&quedan, &c)) {
            conn.execute(
                "UPDATE archivos SET estado = 'pendiente', mtime = NULL WHERE ruta = ?1",
                [ruta],
            )?;
        }
    }
    Ok(())
}
