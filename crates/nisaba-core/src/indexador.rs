use crate::adaptadores::{claude_code, codex};
use crate::modelo::{ConversacionParseada, Fuente};
use crate::titulo::titulo_util;
use crate::{dir_claude, dir_codex, nisaba_home};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Rutas {
    pub home: PathBuf,
    pub claude: PathBuf,
    pub codex: PathBuf,
}

impl Rutas {
    pub fn desde_entorno() -> Rutas {
        Rutas { home: nisaba_home(), claude: dir_claude(), codex: dir_codex() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progreso {
    pub vistos: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultadoIndexado {
    Hecho { nuevos: usize, cambiados: usize, errores: usize },
    /// Otro proceso está indexando (el bloqueo está tomado).
    Ocupado,
    /// Fallo real de E/S al preparar la pasada (carpeta o bloqueo).
    Fallo(String),
}

struct Candidato {
    fuente: Fuente,
    ruta: PathBuf,
    tamano: i64,
    mtime: i64,
}

fn mtime_segs(ruta: &Path) -> Option<(i64, i64)> {
    let m = fs::metadata(ruta).ok()?;
    let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some((m.len() as i64, t))
}

fn huella(c: &ConversacionParseada) -> String {
    let mut h = Sha256::new();
    h.update(c.fuente.as_str().as_bytes());
    h.update(c.id_original.as_bytes());
    h.update(c.cwd.as_deref().unwrap_or("").as_bytes());
    h.update(c.padre_original.as_deref().unwrap_or("").as_bytes());
    h.update(c.hoja.as_deref().unwrap_or("").as_bytes());
    for m in &c.mensajes {
        h.update(format!(
            "|{:?}|{:?}|{:?}|{:?}|{}",
            m.uuid, m.id_padre, m.rol, m.hora, m.oculto
        ));
        for p in &m.partes {
            h.update(p.tipo.as_str().as_bytes());
            h.update(p.contenido.as_bytes());
        }
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn marcar(
    conn: &Connection,
    c: &Candidato,
    estado: &str,
    error: Option<&str>,
    avisos: usize,
    con_firma: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO archivos(ruta, fuente, tamano, mtime, estado, error, avisos, visto)
         VALUES (?1,?2,?3,?4,?5,?6,?7, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(ruta) DO UPDATE SET fuente=excluded.fuente, tamano=excluded.tamano,
           mtime=excluded.mtime, estado=excluded.estado, error=excluded.error,
           avisos=excluded.avisos, visto=excluded.visto",
        params![
            c.ruta.to_string_lossy(),
            c.fuente.as_str(),
            con_firma.then_some(c.tamano),
            con_firma.then_some(c.mtime),
            estado,
            error,
            avisos as i64
        ],
    )?;
    Ok(())
}

enum Escrito {
    Nuevo,
    Cambiado,
    Igual,
    Omitido,
    /// La versión nueva tiene menos mensajes que la indexada: (antes, ahora). No se escribe nada.
    Encogido(i64, i64),
}

/// Escribe la conversación en una transacción. `pendientes` recibe los hijos cuyo padre aún no existe.
fn escribir(
    conn: &mut Connection,
    c: &Candidato,
    conv: &ConversacionParseada,
    pendientes: &mut Vec<(i64, Fuente, String)>,
) -> rusqlite::Result<Escrito> {
    let fuente = conv.fuente.as_str();
    let previo: Option<(i64, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT id, huella, ruta FROM conversaciones WHERE fuente=?1 AND id_original=?2",
            params![fuente, conv.id_original],
            |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?)),
        )
        .optional()?;
    let ruta_txt = c.ruta.to_string_lossy().to_string();
    // Mismo id en otra ruta (Codex sessions/ vs archived_sessions/): gana el mtime mayor.
    if let Some((_, _, Some(otra))) = &previo {
        if *otra != ruta_txt {
            let mt_otra: Option<i64> = conn
                .query_row("SELECT mtime FROM archivos WHERE ruta=?1", [otra], |f| f.get(0))
                .optional()?
                .flatten();
            if mt_otra.is_some_and(|m| m >= c.mtime) {
                return Ok(Escrito::Omitido);
            }
        }
    }
    let nueva = huella(conv);
    if let Some((_, Some(h), Some(r))) = &previo {
        if *h == nueva && *r == ruta_txt {
            return Ok(Escrito::Igual);
        }
    }
    // La tablilla solo crece: si la nueva versión trae menos mensajes, se conserva la indexada.
    if let Some((id_previo, _, _)) = &previo {
        let antes: i64 = conn.query_row(
            "SELECT count(*) FROM mensajes WHERE conversacion_id=?1",
            [id_previo],
            |f| f.get(0),
        )?;
        let ahora = conv.mensajes.len() as i64;
        if ahora < antes {
            return Ok(Escrito::Encogido(antes, ahora));
        }
    }
    let proyecto = conv
        .cwd
        .as_deref()
        .and_then(|d| Path::new(d.trim_end_matches('/')).file_name())
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty());
    let titulo = titulo_util(proyecto.as_deref(), &conv.mensajes);
    let inicio = conv.mensajes.iter().find_map(|m| m.hora.clone());
    let fin = conv.mensajes.iter().rev().find_map(|m| m.hora.clone());

    let tx = conn.transaction()?;
    let padre_id: Option<i64> = match &conv.padre_original {
        Some(p) => tx
            .query_row(
                "SELECT id FROM conversaciones WHERE fuente=?1 AND id_original=?2",
                params![fuente, p],
                |f| f.get(0),
            )
            .optional()?,
        None => None,
    };
    tx.execute(
        "INSERT INTO conversaciones(fuente, id_original, padre_id, descripcion, proyecto, titulo, inicio, fin, hoja, ruta, huella)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(fuente, id_original) DO UPDATE SET padre_id=excluded.padre_id,
           descripcion=excluded.descripcion, proyecto=excluded.proyecto, titulo=excluded.titulo,
           inicio=excluded.inicio, fin=excluded.fin, hoja=excluded.hoja, ruta=excluded.ruta,
           huella=excluded.huella",
        params![
            fuente, conv.id_original, padre_id, conv.descripcion, proyecto, titulo, inicio, fin,
            conv.hoja, ruta_txt, nueva
        ],
    )?;
    let id: i64 = tx.query_row(
        "SELECT id FROM conversaciones WHERE fuente=?1 AND id_original=?2",
        params![fuente, conv.id_original],
        |f| f.get(0),
    )?;
    tx.execute("DELETE FROM mensajes WHERE conversacion_id=?1", [id])?;
    {
        let mut ins_m = tx.prepare(
            "INSERT INTO mensajes(conversacion_id, orden, uuid, id_padre, rol, modelo, hora, oculto)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        )?;
        let mut ins_p = tx.prepare(
            "INSERT INTO partes(mensaje_id, orden, tipo, contenido) VALUES (?1,?2,?3,?4)",
        )?;
        for (i, m) in conv.mensajes.iter().enumerate() {
            ins_m.execute(params![
                id, i as i64, m.uuid, m.id_padre, m.rol.as_str(), m.modelo, m.hora, m.oculto as i64
            ])?;
            let mid = tx.last_insert_rowid();
            for (j, p) in m.partes.iter().enumerate() {
                ins_p.execute(params![mid, j as i64, p.tipo.as_str(), p.contenido])?;
            }
        }
    }
    tx.commit()?;
    if padre_id.is_none() {
        if let Some(p) = &conv.padre_original {
            pendientes.push((id, conv.fuente, p.clone()));
        }
    }
    Ok(if previo.is_some() { Escrito::Cambiado } else { Escrito::Nuevo })
}

/// Segundos Unix → `AAAAMMDDTHHMMSSZ` (UTC, calendario civil).
fn marca_utc(segs: u64) -> String {
    let (dias, resto) = ((segs / 86_400) as i64, segs % 86_400);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", resto / 3_600, resto % 3_600 / 60, resto % 60)
}

fn destino_copia(rutas: &Rutas, fuente: Fuente, origen: &Path) -> Result<PathBuf, String> {
    let (raiz, carpeta) = match fuente {
        Fuente::ClaudeCode => (&rutas.claude, "claude-code"),
        Fuente::Codex => (&rutas.codex, "codex"),
    };
    let rel = origen
        .strip_prefix(raiz)
        .map_err(|e| format!("ruta fuera de la raíz: {e}"))?;
    Ok(rutas.home.join("copias").join(carpeta).join(rel))
}

/// Copia atómica del original a `home/copias/<fuente>/<ruta relativa a la raíz de esa fuente>`.
/// La ruta relativa sale de `descubrir` (strip_prefix de la raíz), nunca del contenido del JSON.
/// Los bytes se copian tal cual (un `.jsonl.zst` sigue siendo `.zst`). Si el original es más pequeño
/// que la copia ya guardada, la copia no se pisa: la versión nueva va al lado como `.encogido-<UTC>`.
fn copiar_original(rutas: &Rutas, fuente: Fuente, origen: &Path) -> Result<(), String> {
    let mut destino = destino_copia(rutas, fuente, origen)?;
    let dir = destino.parent().ok_or("ruta sin carpeta")?.to_path_buf();
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tam_origen = fs::metadata(origen).map_err(|e| format!("{}: {e}", origen.display()))?.len();
    if fs::metadata(&destino).is_ok_and(|m| m.len() > tam_origen) {
        let ahora = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut nombre = destino.file_name().ok_or("ruta sin nombre")?.to_os_string();
        nombre.push(format!(".encogido-{}", marca_utc(ahora)));
        destino = dir.join(nombre);
    }
    let mut tmp = destino.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::copy(origen, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &destino).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", destino.display())
    })
}

pub fn indexar(
    conn: &mut Connection,
    rutas: &Rutas,
    progreso: &mut dyn FnMut(Progreso),
) -> ResultadoIndexado {
    if let Err(e) = fs::create_dir_all(&rutas.home) {
        return ResultadoIndexado::Fallo(format!("{}: {e}", rutas.home.display()));
    }
    let lock = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(rutas.home.join("index.lock"))
    {
        Ok(f) => f,
        Err(e) => return ResultadoIndexado::Fallo(format!("index.lock: {e}")),
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return ResultadoIndexado::Ocupado,
        Err(TryLockError::Error(e)) => return ResultadoIndexado::Fallo(format!("index.lock: {e}")),
    }

    let mut candidatos: Vec<Candidato> = Vec::new();
    let fuentes = [
        (Fuente::ClaudeCode, claude_code::descubrir(&rutas.claude)),
        (Fuente::Codex, codex::descubrir(&rutas.codex)),
    ];
    for (fuente, rutas_f) in fuentes {
        for ruta in rutas_f {
            if let Some((tamano, mtime)) = mtime_segs(&ruta) {
                candidatos.push(Candidato { fuente, ruta, tamano, mtime });
            }
        }
    }
    candidatos.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.ruta.cmp(&b.ruta)));

    let total = candidatos.len();
    let (mut nuevos, mut cambiados, mut errores) = (0, 0, 0);
    let mut pendientes: Vec<(i64, Fuente, String)> = Vec::new();
    let mut ultimo = Instant::now();
    let mut vistos_tamano: HashMap<String, (i64, i64)> = HashMap::new();
    if let Ok(mut st) = conn.prepare("SELECT ruta, tamano, mtime FROM archivos") {
        if let Ok(filas) = st.query_map([], |f| Ok((f.get::<_, String>(0)?, f.get(1)?, f.get(2)?))) {
            for (r, t, m) in filas.flatten() {
                vistos_tamano.insert(r, (t, m));
            }
        }
    }

    for (i, c) in candidatos.iter().enumerate() {
        let clave = c.ruta.to_string_lossy().to_string();
        // Sin cambios solo si además la copia existe: así los archivos ya indexados antes de que
        // existiera la copia (p. ej. Codex) se copian en la siguiente pasada.
        let sin_cambios = vistos_tamano.get(&clave) == Some(&(c.tamano, c.mtime))
            && destino_copia(rutas, c.fuente, &c.ruta).is_ok_and(|d| d.exists());
        if !sin_cambios {
            let fallo_copia = copiar_original(rutas, c.fuente, &c.ruta)
                .err()
                .map(|e| format!("copia: {e}"));
            let firma = fallo_copia.is_none();
            match parsear(c) {
                Err(e) => {
                    errores += 1;
                    let msg = match &fallo_copia {
                        Some(f) => format!("{f}; {e}"),
                        None => e,
                    };
                    let _ = marcar(conn, c, "error", Some(&msg), 0, firma);
                }
                Ok(conv) => match escribir(conn, c, &conv, &mut pendientes) {
                    Ok(r) => {
                        match r {
                            Escrito::Nuevo => nuevos += 1,
                            Escrito::Cambiado => cambiados += 1,
                            Escrito::Igual | Escrito::Omitido | Escrito::Encogido(..) => {}
                        }
                        let encogio = match r {
                            Escrito::Encogido(antes, ahora) => Some(format!(
                                "encogió: {antes}→{ahora} mensajes; la versión anterior se conserva"
                            )),
                            _ => None,
                        };
                        match (&fallo_copia, &encogio) {
                            (Some(f), e) => {
                                let msg = match e {
                                    Some(e) => format!("{f}; {e}"),
                                    None => f.clone(),
                                };
                                errores += 1;
                                let _ = marcar(conn, c, "error", Some(&msg), conv.avisos.len(), false);
                            }
                            (None, Some(e)) => {
                                errores += 1;
                                let _ = marcar(conn, c, "error", Some(e), conv.avisos.len(), true);
                            }
                            (None, None) => {
                                let _ = marcar(conn, c, "ok", None, conv.avisos.len(), true);
                            }
                        }
                    }
                    Err(e) => {
                        errores += 1;
                        let _ = marcar(conn, c, "error", Some(&format!("base de datos: {e}")), 0, firma);
                    }
                },
            }
        }
        if ultimo.elapsed() >= Duration::from_millis(500) {
            progreso(Progreso { vistos: i + 1, total });
            ultimo = Instant::now();
        }
    }

    for (id, fuente, padre) in pendientes {
        let _ = conn.execute(
            "UPDATE conversaciones SET padre_id=(SELECT id FROM conversaciones WHERE fuente=?1 AND id_original=?2) WHERE id=?3",
            params![fuente.as_str(), padre, id],
        );
    }
    progreso(Progreso { vistos: total, total });
    drop(lock);
    ResultadoIndexado::Hecho { nuevos, cambiados, errores }
}

fn parsear(c: &Candidato) -> Result<ConversacionParseada, String> {
    match c.fuente {
        Fuente::ClaudeCode => claude_code::parsear(&c.ruta),
        Fuente::Codex => codex::parsear(&c.ruta),
    }
}

#[cfg(test)]
mod pruebas {
    #[test]
    fn marca_utc_formato() {
        assert_eq!(super::marca_utc(0), "19700101T000000Z");
        assert_eq!(super::marca_utc(1_900_000_000), "20300317T174640Z");
    }
}
