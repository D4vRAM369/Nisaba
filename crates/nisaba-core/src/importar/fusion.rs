//! Reimportar sin perder nada: compara cada conversación del ZIP con la guardada y decide.
//! La tablilla solo crece: lo único que se reescribe es lo que la nueva ya contiene entero.

use crate::indexador::{huella, insertar_mensajes};
use crate::lector::{cargar_mensajes, Msg};
use crate::modelo::{ConversacionImportada, Mensaje};
use crate::titulo::titulo_util;
use rusqlite::{params, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Qué pasó con una conversación: alimenta el resumen de la importación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Efecto {
    Nueva,
    Actualizada,
    Versionada,
    Igual,
}

/// La fila guardada con ese id y sin `version_de`: la que se ve y se busca.
struct Actual {
    id: i64,
    huella: Option<String>,
    titulo: Option<String>,
    proyecto: Option<String>,
    inicio: Option<String>,
    fin: Option<String>,
    tipo: String,
}

/// Los dos `fin` como instantes. `None` si falta la fecha o no se entiende: entonces no se puede
/// afirmar nada, porque un borrado real también trae menos mensajes. Se comparan con `julianday`
/// y no como texto: `10:00:00.5Z` es posterior a `10:00:00Z` aunque ordene antes.
struct Fines {
    entrante: Option<f64>,
    guardado: Option<f64>,
}

impl Fines {
    fn de(tx: &Transaction, actual: &Actual, c: &ConversacionImportada) -> rusqlite::Result<Self> {
        tx.query_row(
            "SELECT julianday(?1), julianday(?2)",
            params![c.fin, actual.fin],
            |f| {
                Ok(Fines {
                    entrante: f.get(0)?,
                    guardado: f.get(1)?,
                })
            },
        )
    }

    fn nuevo(&self) -> bool {
        matches!((self.entrante, self.guardado), (Some(e), Some(g)) if e > g)
    }

    fn viejo(&self) -> bool {
        matches!((self.entrante, self.guardado), (Some(e), Some(g)) if e < g)
    }

    /// Invariante B: el `fin` de la actual solo cambia por uno válido y posterior, o si no tenía.
    fn entrante_gana(&self) -> bool {
        self.nuevo() || (self.entrante.is_some() && self.guardado.is_none())
    }
}

/// Con qué datos se escribe una fila. Lo que el ZIP no trae no borra lo que ya había.
struct Datos<'a> {
    titulo: Option<&'a str>,
    proyecto: Option<&'a str>,
    inicio: Option<&'a str>,
    fin: Option<&'a str>,
}

impl<'a> Datos<'a> {
    fn de(c: &'a ConversacionImportada) -> Self {
        Datos {
            titulo: c.titulo.as_deref(),
            proyecto: c.proyecto.as_deref(),
            inicio: c.inicio.as_deref(),
            fin: c.fin.as_deref(),
        }
    }

    fn sobre(c: &'a ConversacionImportada, actual: &'a Actual, fines: &Fines) -> Self {
        let d = Datos::de(c);
        Datos {
            titulo: d.titulo.or(actual.titulo.as_deref()),
            proyecto: d.proyecto.or(actual.proyecto.as_deref()),
            inicio: d.inicio.or(actual.inicio.as_deref()),
            fin: if fines.entrante_gana() {
                d.fin
            } else {
                actual.fin.as_deref()
            },
        }
    }
}

/// Decide qué hacer con lo que llega, dentro de la transacción de quien llama: la importación
/// entera se confirma o se descarta de una vez. Es una lista: gana la primera regla que encaje,
/// y su orden es lo que impide que un ZIP antiguo pise lo guardado. Además, siempre:
/// A. `originales` solo crece: toda importación deja su JSON en la fila que le corresponde.
/// B. El `fin` de la actual nunca retrocede.
/// C. Un id con `@` se rechaza.
pub fn fusionar(tx: &Transaction, c: &ConversacionImportada) -> rusqlite::Result<Efecto> {
    // `@` marca las versiones (`<id>@<n>`): un ZIP manipulado podría colarse en una de ellas.
    if c.base.id_original.contains('@') {
        let motivo = format!("id de conversación con @: {}", c.base.id_original);
        return Err(rusqlite::Error::ToSqlConversionFailure(motivo.into()));
    }
    let huella = huella_web(c);

    // Regla 1: no hay actual.
    let Some(actual) = buscar_actual(tx, c)? else {
        escribir(tx, c, &c.base.id_original, &Datos::de(c), &huella)?;
        return Ok(Efecto::Nueva);
    };
    let fines = Fines::de(tx, &actual, c)?;
    let version = buscar_version(tx, actual.id, &huella)?;
    let datos = Datos::sobre(c, &actual, &fines);

    // Regla 2: misma huella que la actual.
    if actual.huella.as_deref() == Some(huella.as_str()) {
        anadir_original(tx, actual.id, c)?;
        if fines.viejo() {
            return Ok(Efecto::Igual);
        }
        avanzar_fin(tx, actual.id, c, &fines)?;
        if !datos_distintos(&actual, c) {
            return Ok(Efecto::Igual);
        }
        actualizar_datos(tx, actual.id, c)?;
        return Ok(Efecto::Actualizada);
    }

    // Regla 3: un ZIP viejo nunca toca la actual. Queda como versión, tal como era entonces.
    if fines.viejo() {
        if let Some(version) = version {
            anadir_original(tx, version, c)?;
            return Ok(Efecto::Igual);
        }
        archivar(tx, actual.id, c, &Datos::de(c), &huella)?;
        return Ok(Efecto::Versionada);
    }

    // Regla 4: contenido que ya fue una versión. Solo vuelve si es demostrablemente más nuevo:
    // entonces el usuario ha vuelto a él.
    if let Some(version) = version {
        if fines.nuevo() {
            versionar(tx, actual.id, c, &datos, &huella)?;
            return Ok(Efecto::Versionada);
        }
        anadir_original(tx, version, c)?;
        avanzar_fin(tx, actual.id, c, &fines)?;
        return Ok(Efecto::Igual);
    }

    // Regla 5: la actual sigue entera dentro de lo que llega.
    if solo_anade(&cargar_mensajes(tx, actual.id)?, &c.base.mensajes) {
        escribir(tx, c, &c.base.id_original, &datos, &huella)?;
        return Ok(Efecto::Actualizada);
    }

    // Regla 6: el resto. La actual pasa a versión y entra la nueva.
    versionar(tx, actual.id, c, &datos, &huella)?;
    Ok(Efecto::Versionada)
}

fn buscar_actual(tx: &Transaction, c: &ConversacionImportada) -> rusqlite::Result<Option<Actual>> {
    tx.query_row(
        "SELECT id, huella, titulo, proyecto, inicio, fin, tipo FROM conversaciones
         WHERE fuente=?1 AND id_original=?2",
        params![c.base.fuente.as_str(), c.base.id_original],
        |f| {
            Ok(Actual {
                id: f.get(0)?,
                huella: f.get(1)?,
                titulo: f.get(2)?,
                proyecto: f.get(3)?,
                inicio: f.get(4)?,
                fin: f.get(5)?,
                tipo: f.get(6)?,
            })
        },
    )
    .optional()
}

/// La versión con ese contenido. Puede haber varias si se volvió a él: vale la más antigua.
fn buscar_version(tx: &Transaction, id_actual: i64, huella: &str) -> rusqlite::Result<Option<i64>> {
    tx.query_row(
        "SELECT id FROM conversaciones WHERE version_de = ?1 AND huella = ?2 ORDER BY id LIMIT 1",
        params![id_actual, huella],
        |f| f.get(0),
    )
    .optional()
}

/// Amplía la huella local con lo que aporta la web. No se toca `huella`: cambiarla reindexaría
/// todo lo local. Aquí sí se marcan los límites de cada parte, porque una huella repetida por
/// error haría pasar por `Igual` un contenido distinto y se perdería. Título, proyecto y tipo
/// no entran: son datos de la conversación, no contenido, y se actualizan sin crear versión.
fn huella_web(c: &ConversacionImportada) -> String {
    let mut h = Sha256::new();
    h.update(huella(&c.base));
    for m in &c.base.mensajes {
        h.update(format!("|{:?}", m.modelo));
        for p in &m.partes {
            h.update(format!("|{}", p.contenido.len()));
        }
    }
    hex(h)
}

fn hex(h: Sha256) -> String {
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Un dato ausente no es un cambio: la exportación solo no lo trae.
fn datos_distintos(actual: &Actual, c: &ConversacionImportada) -> bool {
    (c.titulo.is_some() && c.titulo != actual.titulo)
        || (c.proyecto.is_some() && c.proyecto != actual.proyecto)
        || (c.inicio.is_some() && c.inicio != actual.inicio)
        || c.tipo.as_str() != actual.tipo
}

/// Mismo contenido con otros datos: no se tocan los mensajes ni `importada`, que fecha el contenido.
fn actualizar_datos(tx: &Transaction, id: i64, c: &ConversacionImportada) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE conversaciones SET titulo = COALESCE(?1, titulo),
           proyecto = COALESCE(?2, proyecto), inicio = COALESCE(?3, inicio), tipo = ?4
         WHERE id = ?5",
        params![c.titulo, c.proyecto, c.inicio, c.tipo.as_str(), id],
    )?;
    Ok(())
}

fn avanzar_fin(
    tx: &Transaction,
    id: i64,
    c: &ConversacionImportada,
    fines: &Fines,
) -> rusqlite::Result<()> {
    if fines.entrante_gana() {
        tx.execute(
            "UPDATE conversaciones SET fin = ?1 WHERE id = ?2",
            params![c.fin, id],
        )?;
    }
    Ok(())
}

/// `true` si la nueva trae algo más y cada mensaje guardado sigue igual en ella. Contar no basta:
/// una conversación puede crecer y a la vez haber perdido o editado un mensaje anterior.
fn solo_anade(viejos: &[Msg], nuevos: &[Mensaje]) -> bool {
    let por_uuid: HashMap<&str, &Mensaje> = nuevos
        .iter()
        .filter_map(|m| m.uuid.as_deref().map(|u| (u, m)))
        .collect();
    nuevos.len() > viejos.len()
        && viejos.iter().enumerate().all(|(i, v)| {
            let par = match v.uuid.as_deref() {
                Some(u) => por_uuid.get(u).copied(),
                None => nuevos.get(i),
            };
            par.is_some_and(|n| mismo_mensaje(v, n))
        })
}

fn mismo_mensaje(v: &Msg, n: &Mensaje) -> bool {
    v.uuid == n.uuid
        && v.id_padre == n.id_padre
        && v.rol == n.rol.as_str()
        && v.modelo == n.modelo
        && v.hora == n.hora
        && v.oculto == n.oculto
        && v.partes.len() == n.partes.len()
        && v.partes
            .iter()
            .zip(&n.partes)
            .all(|((tipo, contenido), p)| tipo == p.tipo.as_str() && *contenido == p.contenido)
}

/// `<id>@<n>` por orden de llegada: `@1` es la primera que pasó a ser versión.
fn nombre_de_version(
    tx: &Transaction,
    id_vigente: i64,
    c: &ConversacionImportada,
) -> rusqlite::Result<String> {
    let anteriores: i64 = tx.query_row(
        "SELECT count(*) FROM conversaciones WHERE version_de = ?1",
        [id_vigente],
        |f| f.get(0),
    )?;
    Ok(format!("{}@{}", c.base.id_original, anteriores + 1))
}

/// La fila vieja conserva su `id` (y con él mensajes y original); solo cambia de nombre para
/// dejar libre el id limpio.
fn versionar(
    tx: &Transaction,
    id_previo: i64,
    c: &ConversacionImportada,
    datos: &Datos,
    huella: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE conversaciones SET id_original = ?1 WHERE id = ?2",
        params![nombre_de_version(tx, id_previo, c)?, id_previo],
    )?;
    let id = escribir(tx, c, &c.base.id_original, datos, huella)?;
    tx.execute(
        "UPDATE conversaciones SET version_de = ?1 WHERE id = ?2 OR version_de = ?2",
        params![id, id_previo],
    )?;
    Ok(())
}

/// Al revés que `versionar`: la vigente no se toca y lo que llega entra ya como versión suya.
fn archivar(
    tx: &Transaction,
    id_vigente: i64,
    c: &ConversacionImportada,
    datos: &Datos,
    huella: &str,
) -> rusqlite::Result<()> {
    let nombre = nombre_de_version(tx, id_vigente, c)?;
    let id = escribir(tx, c, &nombre, datos, huella)?;
    tx.execute(
        "UPDATE conversaciones SET version_de = ?1 WHERE id = ?2",
        params![id_vigente, id],
    )?;
    Ok(())
}

/// Crea la fila `id_original` o, si existe, la sustituye por la nueva. Título y fechas que no
/// vengan en `datos` se deducen de los mensajes.
fn escribir(
    tx: &Transaction,
    c: &ConversacionImportada,
    id_original: &str,
    datos: &Datos,
    huella: &str,
) -> rusqlite::Result<i64> {
    let conv = &c.base;
    let proyecto = datos.proyecto;
    let titulo = datos
        .titulo
        .map(str::to_string)
        .unwrap_or_else(|| titulo_util(proyecto, &conv.mensajes));
    let inicio = datos
        .inicio
        .or_else(|| conv.mensajes.iter().find_map(|m| m.hora.as_deref()));
    let fin = datos
        .fin
        .or_else(|| conv.mensajes.iter().rev().find_map(|m| m.hora.as_deref()));
    let id: i64 = tx.query_row(
        "INSERT INTO conversaciones(fuente, id_original, descripcion, proyecto, titulo, inicio, fin,
           hoja, huella, cwd, tipo, importada)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(fuente, id_original) DO UPDATE SET descripcion=excluded.descripcion,
           proyecto=excluded.proyecto, titulo=excluded.titulo, inicio=excluded.inicio,
           fin=excluded.fin, hoja=excluded.hoja, huella=excluded.huella, cwd=excluded.cwd,
           tipo=excluded.tipo, importada=excluded.importada
         RETURNING id",
        params![
            conv.fuente.as_str(),
            id_original,
            conv.descripcion,
            proyecto,
            titulo,
            inicio,
            fin,
            conv.hoja,
            huella,
            conv.cwd,
            c.tipo.as_str()
        ],
        |f| f.get(0),
    )?;
    tx.execute("DELETE FROM mensajes WHERE conversacion_id = ?1", [id])?;
    insertar_mensajes(tx, id, conv)?;
    anadir_original(tx, id, c)?;
    Ok(id)
}

/// `originales` es de solo añadir: el mismo JSON en la misma fila no se repite y nada se pisa.
fn anadir_original(tx: &Transaction, id: i64, c: &ConversacionImportada) -> rusqlite::Result<()> {
    if let Some(json) = &c.original {
        let mut h = Sha256::new();
        h.update(json);
        tx.execute(
            "INSERT INTO originales(conversacion_id, huella, json) VALUES (?1, ?2, ?3)
             ON CONFLICT(conversacion_id, huella) DO NOTHING",
            params![id, hex(h), json],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archivo_web;
    use crate::boveda::LlaveMaestra;
    use crate::busqueda::{self, Filtros, Resolucion};
    use crate::lector::{self, Bloque, Tramo};
    use crate::modelo::{ConversacionParseada, Fuente, Mensaje, Parte, Rol, TipoParte, TipoWeb};
    use rusqlite::Connection;
    use tempfile::{tempdir, TempDir};

    const ID: &str = "aaaa1111-2222-3333-4444-555566667777";

    fn base() -> (TempDir, Connection) {
        let dir = tempdir().unwrap();
        let con = archivo_web::abrir(dir.path(), &LlaveMaestra::de_prueba(40)).unwrap();
        (dir, con)
    }

    /// Mensaje suelto: sin uuid se identifica por su posición.
    fn m(texto: &str) -> Mensaje {
        Mensaje {
            uuid: None,
            id_padre: None,
            rol: Rol::Usuario,
            modelo: None,
            hora: None,
            oculto: false,
            partes: vec![Parte {
                tipo: TipoParte::Texto,
                contenido: texto.to_string(),
            }],
        }
    }

    fn con_uuid(uuid: &str, texto: &str) -> Mensaje {
        Mensaje {
            uuid: Some(uuid.to_string()),
            ..m(texto)
        }
    }

    fn conv(mensajes: Vec<Mensaje>) -> ConversacionImportada {
        ConversacionImportada {
            base: ConversacionParseada {
                fuente: Fuente::ClaudeWeb,
                id_original: ID.to_string(),
                cwd: None,
                padre_original: None,
                descripcion: None,
                hoja: None,
                mensajes,
                avisos: vec![],
            },
            titulo: Some("Plan de riego".to_string()),
            proyecto: None,
            inicio: Some("2026-10-01T10:00:00Z".to_string()),
            fin: Some("2026-10-01T11:00:00Z".to_string()),
            tipo: TipoWeb::Conversacion,
            original: None,
        }
    }

    fn importar(con: &mut Connection, c: &ConversacionImportada) -> Efecto {
        let tx = con.transaction().unwrap();
        let e = fusionar(&tx, c).unwrap();
        tx.commit().unwrap();
        e
    }

    fn id_de(con: &Connection, id_original: &str) -> i64 {
        con.query_row(
            "SELECT id FROM conversaciones WHERE fuente='claude-web' AND id_original=?1",
            [id_original],
            |f| f.get(0),
        )
        .unwrap()
    }

    fn textos(con: &Connection, id: i64) -> Vec<String> {
        let mut q = con
            .prepare(
                "SELECT p.contenido FROM partes p JOIN mensajes m ON m.id = p.mensaje_id
                 WHERE m.conversacion_id = ?1 ORDER BY m.orden, p.orden",
            )
            .unwrap();
        q.query_map([id], |f| f.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn filas(con: &Connection) -> i64 {
        con.query_row("SELECT count(*) FROM conversaciones", [], |f| f.get(0))
            .unwrap()
    }

    fn resultados(con: &Connection, q: &str) -> Vec<String> {
        busqueda::buscar_sin_contar(con, q, &Filtros::default())
            .unwrap()
            .resultados
            .into_iter()
            .map(|r| r.id_original)
            .collect()
    }

    fn originales(con: &Connection, id: i64) -> Vec<String> {
        let mut q = con
            .prepare("SELECT json FROM originales WHERE conversacion_id = ?1 ORDER BY id")
            .unwrap();
        q.query_map([id], |f| f.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn total_originales(con: &Connection) -> i64 {
        con.query_row("SELECT count(*) FROM originales", [], |f| f.get(0))
            .unwrap()
    }

    /// Lo que tiene que quedar tras versionar: la nueva con el id limpio y la anterior intacta.
    fn assert_versionada(con: &Connection, viejos: &[&str], nuevos: &[&str]) {
        let id = id_de(con, ID);
        assert_eq!(textos(con, id), nuevos);
        let vers = lector::versiones(con, id).unwrap();
        assert_eq!(vers.len(), 1);
        assert_eq!(textos(con, vers[0].id), viejos);
        assert_eq!(id_de(con, &format!("{ID}@1")), vers[0].id);
    }

    #[test]
    fn nueva_se_anade() {
        let (_d, mut con) = base();
        assert_eq!(importar(&mut con, &conv(vec![m("hola")])), Efecto::Nueva);
        let id = id_de(&con, ID);
        assert_eq!(textos(&con, id), ["hola"]);
        let (titulo, tipo, importada): (String, String, String) = con
            .query_row(
                "SELECT titulo, tipo, importada FROM conversaciones WHERE id = ?1",
                [id],
                |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?)),
            )
            .unwrap();
        assert_eq!(titulo, "Plan de riego");
        assert_eq!(tipo, "conversacion");
        assert!(
            importada.ends_with('Z') && importada.contains('T'),
            "{importada}"
        );
    }

    #[test]
    fn sin_titulo_ni_fechas_se_deducen_de_los_mensajes() {
        let (_d, mut con) = base();
        let mut c = conv(vec![Mensaje {
            hora: Some("2026-10-02T08:00:00Z".to_string()),
            ..m("cómo se poda un olivo joven")
        }]);
        c.titulo = None;
        c.inicio = None;
        c.fin = None;
        importar(&mut con, &c);
        let (titulo, inicio, fin): (String, String, String) = con
            .query_row("SELECT titulo, inicio, fin FROM conversaciones", [], |f| {
                Ok((f.get(0)?, f.get(1)?, f.get(2)?))
            })
            .unwrap();
        assert!(titulo.contains("olivo"), "{titulo}");
        assert_eq!(inicio, "2026-10-02T08:00:00Z");
        assert_eq!(fin, "2026-10-02T08:00:00Z");
    }

    #[test]
    fn igual_no_toca_nada() {
        let (_d, mut con) = base();
        let mut c = conv(vec![m("hola"), m("adiós")]);
        c.original = Some("{\"v\":1}".to_string());
        importar(&mut con, &c);
        con.execute("UPDATE conversaciones SET importada = 'marca'", [])
            .unwrap();
        let mids = |con: &Connection| -> Vec<i64> {
            let mut q = con.prepare("SELECT id FROM mensajes ORDER BY id").unwrap();
            q.query_map([], |f| f.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let antes = mids(&con);
        assert_eq!(importar(&mut con, &c), Efecto::Igual);
        assert_eq!(filas(&con), 1);
        assert_eq!(mids(&con), antes);
        let marca: String = con
            .query_row("SELECT importada FROM conversaciones", [], |f| f.get(0))
            .unwrap();
        assert_eq!(marca, "marca");
    }

    #[test]
    fn mas_mensajes_actualiza() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("hola")]));
        let id = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &conv(vec![m("hola"), m("cebolla")])),
            Efecto::Actualizada
        );
        assert_eq!(filas(&con), 1);
        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["hola", "cebolla"]);
        assert!(lector::versiones(&con, id).unwrap().is_empty());
        // Sin restos en el índice: cada texto aparece una sola vez.
        let n: i64 = con
            .query_row(
                "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH 'hola'",
                [],
                |f| f.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(resultados(&con, "cebolla"), [ID]);
    }

    #[test]
    fn actualizar_pone_al_dia_los_datos_de_la_conversacion() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("hola")]));
        con.execute("UPDATE conversaciones SET importada = 'marca'", [])
            .unwrap();
        let mut c = conv(vec![m("hola"), m("otra")]);
        c.titulo = Some("Riego por goteo".to_string());
        c.proyecto = Some("Huerto".to_string());
        c.fin = Some("2026-10-05T09:00:00Z".to_string());
        c.tipo = TipoWeb::Diseno;
        c.base.hoja = Some("h2".to_string());
        assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
        let fila: [String; 6] = con
            .query_row(
                "SELECT titulo, proyecto, fin, tipo, hoja, importada FROM conversaciones",
                [],
                |f| {
                    Ok([
                        f.get(0)?,
                        f.get(1)?,
                        f.get(2)?,
                        f.get(3)?,
                        f.get(4)?,
                        f.get(5)?,
                    ])
                },
            )
            .unwrap();
        assert_eq!(
            fila[..5],
            [
                "Riego por goteo",
                "Huerto",
                "2026-10-05T09:00:00Z",
                "diseno",
                "h2"
            ]
        );
        assert_ne!(fila[5], "marca");
        // Y volver a importar lo mismo ya no es un cambio.
        assert_eq!(importar(&mut con, &c), Efecto::Igual);
    }

    #[test]
    fn por_uuid_un_mensaje_intercalado_actualiza() {
        let (_d, mut con) = base();
        importar(
            &mut con,
            &conv(vec![con_uuid("u1", "uno"), con_uuid("u2", "dos")]),
        );
        let c = conv(vec![
            con_uuid("u1", "uno"),
            con_uuid("u3", "rama nueva"),
            con_uuid("u2", "dos"),
        ]);
        assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
        assert_eq!(filas(&con), 1);
    }

    #[test]
    fn sin_uuid_un_mensaje_intercalado_versiona() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("uno"), m("dos")]));
        let c = conv(vec![m("uno"), m("intercalado"), m("dos")]);
        assert_eq!(importar(&mut con, &c), Efecto::Versionada);
        assert_versionada(&con, &["uno", "dos"], &["uno", "intercalado", "dos"]);
    }

    #[test]
    fn menos_mensajes_versiona() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("uno"), m("dos")]));
        assert_eq!(
            importar(&mut con, &conv(vec![m("uno")])),
            Efecto::Versionada
        );
        assert_versionada(&con, &["uno", "dos"], &["uno"]);
    }

    #[test]
    fn texto_distinto_versiona() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("uno"), m("dos")]));
        assert_eq!(
            importar(&mut con, &conv(vec![m("uno"), m("DOS")])),
            Efecto::Versionada
        );
        assert_versionada(&con, &["uno", "dos"], &["uno", "DOS"]);
    }

    #[test]
    fn crecer_perdiendo_un_mensaje_versiona() {
        let (_d, mut con) = base();
        importar(
            &mut con,
            &conv(vec![con_uuid("u1", "uno"), con_uuid("u2", "dos")]),
        );
        // Hay más mensajes que antes, pero `u2` ya no está: contar no basta.
        let c = conv(vec![
            con_uuid("u1", "uno"),
            con_uuid("u3", "tres"),
            con_uuid("u4", "cuatro"),
        ]);
        assert_eq!(importar(&mut con, &c), Efecto::Versionada);
        assert_versionada(&con, &["uno", "dos"], &["uno", "tres", "cuatro"]);
    }

    #[test]
    fn crecer_editando_un_mensaje_versiona() {
        let (_d, mut con) = base();
        importar(
            &mut con,
            &conv(vec![con_uuid("u1", "uno"), con_uuid("u2", "dos")]),
        );
        let c = conv(vec![
            con_uuid("u1", "uno"),
            con_uuid("u2", "dos editado"),
            con_uuid("u3", "tres"),
        ]);
        assert_eq!(importar(&mut con, &c), Efecto::Versionada);
        assert_versionada(&con, &["uno", "dos"], &["uno", "dos editado", "tres"]);
    }

    #[test]
    fn crecer_cambiando_cualquier_campo_de_un_mensaje_versiona() {
        let cambios: [fn(&mut Mensaje); 7] = [
            |x| x.id_padre = Some("otro".to_string()),
            |x| x.rol = Rol::Asistente,
            |x| x.modelo = Some("otro".to_string()),
            |x| x.hora = Some("2026-01-01T00:00:00Z".to_string()),
            |x| x.oculto = true,
            |x| x.partes[0].tipo = TipoParte::Razonamiento,
            |x| x.partes.push(m("parte añadida").partes.remove(0)),
        ];
        for cambio in cambios {
            let (_d, mut con) = base();
            importar(&mut con, &conv(vec![con_uuid("u1", "uno")]));
            let mut editado = con_uuid("u1", "uno");
            cambio(&mut editado);
            let c = conv(vec![editado, con_uuid("u2", "dos")]);
            assert_eq!(importar(&mut con, &c), Efecto::Versionada);
        }
    }

    #[test]
    fn cambio_de_titulo_no_crea_version() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("uno")]));
        let id = id_de(&con, ID);
        con.execute("UPDATE conversaciones SET importada = 'marca'", [])
            .unwrap();
        let mensaje: i64 = con
            .query_row("SELECT id FROM mensajes", [], |f| f.get(0))
            .unwrap();

        let mut c = conv(vec![m("uno")]);
        c.titulo = Some("Otro título".to_string());
        c.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &c), Efecto::Actualizada);

        assert_eq!(filas(&con), 1);
        assert_eq!(id_de(&con, ID), id);
        assert!(lector::versiones(&con, id).unwrap().is_empty());
        let (titulo, importada): (String, String) = con
            .query_row("SELECT titulo, importada FROM conversaciones", [], |f| {
                Ok((f.get(0)?, f.get(1)?))
            })
            .unwrap();
        assert_eq!(titulo, "Otro título");
        // El contenido no ha cambiado: ni su fecha de entrada ni sus mensajes se rehacen.
        assert_eq!(importada, "marca");
        let mismo: i64 = con
            .query_row("SELECT id FROM mensajes", [], |f| f.get(0))
            .unwrap();
        assert_eq!(mismo, mensaje);
        assert_eq!(originales(&con, id), ["{\"v\":2}"]);
        assert_eq!(importar(&mut con, &c), Efecto::Igual);
    }

    #[test]
    fn cambio_de_proyecto_o_tipo_actualiza_sin_version() {
        let ediciones: [fn(&mut ConversacionImportada); 2] = [
            |c| c.proyecto = Some("Huerto".to_string()),
            |c| c.tipo = TipoWeb::Documento,
        ];
        for editar in ediciones {
            let (_d, mut con) = base();
            importar(&mut con, &conv(vec![m("uno")]));
            let mut c = conv(vec![m("uno")]);
            editar(&mut c);
            assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
            assert_eq!(filas(&con), 1);
            assert_eq!(importar(&mut con, &c), Efecto::Igual);
        }
    }

    #[test]
    fn modelo_entra_en_la_huella() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("uno")]));
        let mut c = conv(vec![m("uno")]);
        c.base.mensajes[0].modelo = Some("modelo-x".to_string());
        assert_eq!(importar(&mut con, &c), Efecto::Versionada);
    }

    #[test]
    fn sin_proyecto_no_borra_el_proyecto() {
        let datos = |con: &Connection| -> (String, String) {
            con.query_row(
                "SELECT titulo, proyecto FROM conversaciones WHERE id_original = ?1",
                [ID],
                |f| Ok((f.get(0)?, f.get(1)?)),
            )
            .unwrap()
        };
        let esperados = ("Plan de riego".to_string(), "Huerto".to_string());
        let sin_datos = |mensajes| {
            let mut c = conv(mensajes);
            c.titulo = None;
            c.proyecto = None;
            c
        };
        let (_d, mut con) = base();
        let mut a = conv(vec![m("uno")]);
        a.proyecto = Some("Huerto".to_string());
        importar(&mut con, &a);

        // Mismo contenido, más mensajes y contenido distinto: en ninguno se pierden.
        assert_eq!(
            importar(&mut con, &sin_datos(vec![m("uno")])),
            Efecto::Igual
        );
        assert_eq!(datos(&con), esperados);
        assert_eq!(
            importar(&mut con, &sin_datos(vec![m("uno"), m("dos")])),
            Efecto::Actualizada
        );
        assert_eq!(datos(&con), esperados);
        assert_eq!(
            importar(&mut con, &sin_datos(vec![m("otra cosa")])),
            Efecto::Versionada
        );
        assert_eq!(datos(&con), esperados);
    }

    #[test]
    fn partes_que_al_unirse_dan_lo_mismo_no_son_iguales() {
        let (_d, mut con) = base();
        let parte = |t: &str| Parte {
            tipo: TipoParte::Texto,
            contenido: t.to_string(),
        };
        let mut una = m("");
        una.partes = vec![parte("abtextocd")];
        let mut dos = m("");
        dos.partes = vec![parte("ab"), parte("cd")];
        importar(&mut con, &conv(vec![una]));
        assert_eq!(importar(&mut con, &conv(vec![dos])), Efecto::Versionada);
        assert_versionada(&con, &["abtextocd"], &["ab", "cd"]);
    }

    #[test]
    fn sin_confirmar_la_transaccion_no_cambia_nada() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("uno"), m("dos")]));
        let id = id_de(&con, ID);
        let tx = con.transaction().unwrap();
        assert_eq!(
            fusionar(&tx, &conv(vec![m("otra cosa")])).unwrap(),
            Efecto::Versionada
        );
        drop(tx);
        assert_eq!(filas(&con), 1);
        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["uno", "dos"]);
    }

    #[test]
    fn reimportar_zip_antiguo_no_cambia_la_actual() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("antigua")]));
        importar(&mut con, &conv(vec![m("actual")]));
        let id = id_de(&con, ID);
        for _ in 0..2 {
            assert_eq!(importar(&mut con, &conv(vec![m("antigua")])), Efecto::Igual);
            assert_eq!(id_de(&con, ID), id);
            assert_eq!(textos(&con, id), ["actual"]);
            assert_eq!(lector::versiones(&con, id).unwrap().len(), 1);
            assert_eq!(filas(&con), 2);
        }
        assert_eq!(resultados(&con, "actual"), [ID]);
    }

    #[test]
    fn leer_version_abre_una_antigua_y_rechaza_la_actual() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("antigua")]));
        importar(&mut con, &conv(vec![m("actual")]));
        let id = id_de(&con, ID);
        let vieja = lector::versiones(&con, id).unwrap()[0].id;

        let l = lector::leer_version(&con, vieja, &Tramo::default()).unwrap();
        assert_eq!(l.id_original, format!("{ID}@1"));
        assert_eq!(
            l.bloques,
            [Bloque::Usuario {
                hora: None,
                texto: "antigua".to_string()
            }]
        );
        assert!(lector::leer_version(&con, id, &Tramo::default()).is_err());
        assert!(lector::leer_version(&con, 9_999, &Tramo::default()).is_err());
        assert!(lector::leer(&con, vieja, &Tramo::default()).is_err());
    }

    #[test]
    fn id_con_arroba_se_rechaza() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("antigua")]));
        importar(&mut con, &conv(vec![m("actual")]));
        let vieja = id_de(&con, &format!("{ID}@1"));
        for id in [format!("{ID}@1"), "nueva@ejemplo".to_string()] {
            let mut c = conv(vec![m("colada")]);
            c.base.id_original = id;
            let tx = con.transaction().unwrap();
            assert!(fusionar(&tx, &c).is_err());
            drop(tx);
        }
        assert_eq!(filas(&con), 2);
        assert_eq!(textos(&con, vieja), ["antigua"]);
    }

    #[test]
    fn igual_rellena_original_ausente() {
        let (_d, mut con) = base();
        let sin_original = conv(vec![m("uno")]);
        importar(&mut con, &sin_original);
        let id = id_de(&con, ID);
        let mut con_original = sin_original.clone();
        con_original.original = Some("{\"v\":1}".to_string());
        assert_eq!(importar(&mut con, &con_original), Efecto::Igual);
        assert_eq!(originales(&con, id), ["{\"v\":1}"]);

        // El que ya estaba no se pisa: el nuevo se suma.
        con_original.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &con_original), Efecto::Igual);
        assert_eq!(originales(&con, id), ["{\"v\":1}", "{\"v\":2}"]);
    }

    #[test]
    fn igual_a_una_version_rellena_su_original_y_no_el_de_la_actual() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("antigua")]));
        let vieja = id_de(&con, ID);
        importar(&mut con, &conv(vec![m("actual")]));
        let mut c = conv(vec![m("antigua")]);
        c.original = Some("{\"v\":1}".to_string());
        assert_eq!(importar(&mut con, &c), Efecto::Igual);
        assert_eq!(originales(&con, vieja), ["{\"v\":1}"]);
        assert!(originales(&con, id_de(&con, ID)).is_empty());
    }

    fn fin_de(con: &Connection, id: i64) -> Option<String> {
        con.query_row("SELECT fin FROM conversaciones WHERE id = ?1", [id], |f| {
            f.get(0)
        })
        .unwrap()
    }

    fn con_fin(mensajes: Vec<Mensaje>, fin: Option<&str>) -> ConversacionImportada {
        let mut c = conv(mensajes);
        c.fin = fin.map(str::to_string);
        c
    }

    #[test]
    fn zip_antiguo_tras_crecer_no_retrocede() {
        let (_d, mut con) = base();
        let antes = Some("2026-10-01T10:00:00Z");
        let despues = Some("2026-10-02T10:00:00Z");
        importar(&mut con, &con_fin(vec![m("uno")], antes));
        let mut crecida = con_fin(vec![m("uno"), m("dos")], despues);
        crecida.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &crecida), Efecto::Actualizada);
        let id = id_de(&con, ID);

        let mut antigua = con_fin(vec![m("uno")], antes);
        antigua.original = Some("{\"v\":1}".to_string());
        assert_eq!(importar(&mut con, &antigua), Efecto::Versionada);

        // La vigente no se ha movido: mismo id, mismo contenido, misma fecha y mismo original.
        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["uno", "dos"]);
        assert_eq!(resultados(&con, "dos"), [ID]);
        assert_eq!(originales(&con, id), ["{\"v\":2}"]);
        let fin: String = con
            .query_row("SELECT fin FROM conversaciones WHERE id = ?1", [id], |f| {
                f.get(0)
            })
            .unwrap();
        assert_eq!(fin, "2026-10-02T10:00:00Z");

        // La antigua queda como versión, con su nombre, su original y legible.
        let vers = lector::versiones(&con, id).unwrap();
        assert_eq!(vers.len(), 1);
        assert_eq!(id_de(&con, &format!("{ID}@1")), vers[0].id);
        assert_eq!(textos(&con, vers[0].id), ["uno"]);
        assert_eq!(originales(&con, vers[0].id), ["{\"v\":1}"]);
        assert!(lector::leer_version(&con, vers[0].id, &Tramo::default()).is_ok());

        // Una segunda pasada del mismo ZIP ya no añade nada.
        assert_eq!(importar(&mut con, &antigua), Efecto::Igual);
        assert_eq!(lector::versiones(&con, id).unwrap().len(), 1);
    }

    #[test]
    fn version_antigua_sigue_a_la_vigente_si_luego_hay_otra() {
        let (_d, mut con) = base();
        let despues = Some("2026-10-02T10:00:00Z");
        importar(&mut con, &con_fin(vec![m("vigente")], despues));
        let antigua = con_fin(vec![m("antigua")], Some("2026-10-01T10:00:00Z"));
        importar(&mut con, &antigua);
        let nueva = con_fin(vec![m("nueva")], Some("2026-10-03T10:00:00Z"));
        assert_eq!(importar(&mut con, &nueva), Efecto::Versionada);
        let id = id_de(&con, ID);
        assert_eq!(textos(&con, id), ["nueva"]);
        assert_eq!(lector::versiones(&con, id).unwrap().len(), 2);
        assert_eq!(textos(&con, id_de(&con, &format!("{ID}@1"))), ["antigua"]);
        assert_eq!(textos(&con, id_de(&con, &format!("{ID}@2"))), ["vigente"]);
    }

    #[test]
    fn borrado_real_con_fecha_nueva_si_es_la_actual() {
        let (_d, mut con) = base();
        let inicial = con_fin(vec![m("uno"), m("dos")], Some("2026-10-01T10:00:00Z"));
        importar(&mut con, &inicial);
        let borrado = con_fin(vec![m("uno")], Some("2026-10-02T10:00:00Z"));
        assert_eq!(importar(&mut con, &borrado), Efecto::Versionada);
        assert_versionada(&con, &["uno", "dos"], &["uno"]);
    }

    #[test]
    fn sin_fecha_comparable_la_nueva_es_la_actual() {
        let fecha = Some("2026-10-01T10:00:00Z");
        let casos = [
            (fecha, None),
            (None, fecha),
            (None, None),
            (fecha, Some("ayer")),
        ];
        for (vieja, nueva) in casos {
            let (_d, mut con) = base();
            importar(&mut con, &con_fin(vec![m("uno"), m("dos")], vieja));
            let c = con_fin(vec![m("uno")], nueva);
            assert_eq!(importar(&mut con, &c), Efecto::Versionada);
            assert_versionada(&con, &["uno", "dos"], &["uno"]);
        }
    }

    #[test]
    fn las_fechas_se_comparan_como_fechas_y_no_como_texto() {
        // Las dos son posteriores a la guardada, aunque como texto vayan antes.
        for nueva in ["2026-10-01T10:00:00.500Z", "2026-10-01T09:30:00-01:00"] {
            let (_d, mut con) = base();
            let inicial = con_fin(vec![m("uno"), m("dos")], Some("2026-10-01T10:00:00Z"));
            importar(&mut con, &inicial);
            let c = con_fin(vec![m("uno")], Some(nueva));
            assert_eq!(importar(&mut con, &c), Efecto::Versionada);
            assert_versionada(&con, &["uno", "dos"], &["uno"]);
        }
    }

    #[test]
    fn zip_antiguo_con_otro_titulo_no_pisa_el_original() {
        let (_d, mut con) = base();
        let mut nueva = con_fin(vec![m("uno")], Some("2026-10-02T10:00:00Z"));
        nueva.titulo = Some("Título nuevo".to_string());
        nueva.original = Some("{\"v\":2}".to_string());
        importar(&mut con, &nueva);
        let id = id_de(&con, ID);

        let mut antigua = con_fin(vec![m("uno")], Some("2026-10-01T10:00:00Z"));
        antigua.titulo = Some("Título viejo".to_string());
        antigua.proyecto = Some("Viejo".to_string());
        antigua.tipo = TipoWeb::Documento;
        antigua.original = Some("{\"v\":1}".to_string());
        assert_eq!(importar(&mut con, &antigua), Efecto::Igual);

        let fila: (String, Option<String>, String, String) = con
            .query_row(
                "SELECT titulo, proyecto, tipo, fin FROM conversaciones",
                [],
                |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?, f.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            fila,
            (
                "Título nuevo".to_string(),
                None,
                "conversacion".to_string(),
                "2026-10-02T10:00:00Z".to_string()
            )
        );
        // Sus datos no se aplican, pero su JSON se guarda junto al de la actual.
        assert_eq!(originales(&con, id), ["{\"v\":2}", "{\"v\":1}"]);
    }

    #[test]
    fn zip_antiguo_con_mas_mensajes_se_archiva() {
        let (_d, mut con) = base();
        let mut nueva = con_fin(vec![m("uno")], Some("2026-10-02T10:00:00Z"));
        nueva.original = Some("{\"v\":2}".to_string());
        importar(&mut con, &nueva);
        let id = id_de(&con, ID);

        let mut antigua = con_fin(vec![m("uno"), m("dos")], Some("2026-10-01T10:00:00Z"));
        antigua.original = Some("{\"v\":1}".to_string());
        assert_eq!(importar(&mut con, &antigua), Efecto::Versionada);

        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["uno"]);
        assert_eq!(fin_de(&con, id).as_deref(), Some("2026-10-02T10:00:00Z"));
        assert_eq!(originales(&con, id), ["{\"v\":2}"]);
        let vers = lector::versiones(&con, id).unwrap();
        assert_eq!(vers.len(), 1);
        assert_eq!(textos(&con, vers[0].id), ["uno", "dos"]);
        assert_eq!(originales(&con, vers[0].id), ["{\"v\":1}"]);

        assert_eq!(importar(&mut con, &antigua), Efecto::Igual);
        assert_eq!(lector::versiones(&con, id).unwrap().len(), 1);
    }

    #[test]
    fn borrado_real_nuevo_igual_a_una_version_si_es_la_actual() {
        let (_d, mut con) = base();
        let (uno, dos, tres) = (
            "2026-10-01T10:00:00Z",
            "2026-10-02T10:00:00Z",
            "2026-10-03T10:00:00Z",
        );
        importar(&mut con, &con_fin(vec![m("corta")], Some(uno)));
        importar(&mut con, &con_fin(vec![m("corta"), m("larga")], Some(dos)));
        importar(&mut con, &con_fin(vec![m("otra")], Some(tres)));
        // La vigente es `otra`; `@1` guarda corta + larga.
        let atras = con_fin(vec![m("corta"), m("larga")], Some(dos));
        assert_eq!(importar(&mut con, &atras), Efecto::Igual);
        assert_eq!(textos(&con, id_de(&con, ID)), ["otra"]);

        // Misma fecha o sin fecha no demuestran que sea más nueva.
        for fin in [Some(tres), None] {
            let c = con_fin(vec![m("corta"), m("larga")], fin);
            assert_eq!(importar(&mut con, &c), Efecto::Igual);
        }
        assert_eq!(textos(&con, id_de(&con, ID)), ["otra"]);

        // Más nueva que la vigente: el usuario volvió a ese contenido.
        let vuelta = con_fin(vec![m("corta"), m("larga")], Some("2026-10-04T10:00:00Z"));
        assert_eq!(importar(&mut con, &vuelta), Efecto::Versionada);
        let id = id_de(&con, ID);
        assert_eq!(textos(&con, id), ["corta", "larga"]);
        assert_eq!(lector::versiones(&con, id).unwrap().len(), 2);
        assert_eq!(textos(&con, id_de(&con, &format!("{ID}@2"))), ["otra"]);
        assert_eq!(importar(&mut con, &vuelta), Efecto::Igual);
    }

    #[test]
    fn crecer_sin_fecha_no_borra_fin() {
        let (_d, mut con) = base();
        let (inicio, fin) = ("2026-10-01T10:00:00Z", "2026-10-02T10:00:00Z");
        importar(&mut con, &conv(vec![m("uno")]));
        con.execute(
            "UPDATE conversaciones SET inicio = ?1, fin = ?2",
            [inicio, fin],
        )
        .unwrap();
        let fechas = |con: &Connection| -> (String, String) {
            con.query_row(
                "SELECT inicio, fin FROM conversaciones WHERE id_original = ?1",
                [ID],
                |f| Ok((f.get(0)?, f.get(1)?)),
            )
            .unwrap()
        };
        let esperadas = (inicio.to_string(), fin.to_string());
        let sin_fechas = |mensajes| {
            let mut c = conv(mensajes);
            c.inicio = None;
            c.fin = None;
            c
        };
        // La hora de un mensaje no sustituye a la fecha guardada.
        let tardio = Mensaje {
            hora: Some("2026-10-09T10:00:00Z".to_string()),
            ..m("dos")
        };
        assert_eq!(
            importar(&mut con, &sin_fechas(vec![m("uno"), tardio])),
            Efecto::Actualizada
        );
        assert_eq!(fechas(&con), esperadas);
        // Tampoco se pierde al versionar: sin ella, un ZIP viejo ya no se detectaría.
        assert_eq!(
            importar(&mut con, &sin_fechas(vec![m("otra cosa")])),
            Efecto::Versionada
        );
        assert_eq!(fechas(&con), esperadas);
    }

    #[test]
    fn ausente_se_conserva() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("sigo aquí")]));
        let mut otra = conv(vec![m("otra cosa")]);
        otra.base.id_original = "bbbb".to_string();
        assert_eq!(importar(&mut con, &otra), Efecto::Nueva);
        assert_eq!(textos(&con, id_de(&con, ID)), ["sigo aquí"]);
        assert_eq!(filas(&con), 2);
    }

    #[test]
    fn misma_id_en_otra_fuente_es_otra_conversacion() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("en claude")]));
        let mut otra = conv(vec![m("en chatgpt")]);
        otra.base.fuente = Fuente::ChatGpt;
        assert_eq!(importar(&mut con, &otra), Efecto::Nueva);
        assert_eq!(textos(&con, id_de(&con, ID)), ["en claude"]);
    }

    #[test]
    fn tres_importaciones_dejan_dos_versiones() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("primera")]));
        let id_1 = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &conv(vec![m("segunda")])),
            Efecto::Versionada
        );
        let id_2 = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &conv(vec![m("tercera")])),
            Efecto::Versionada
        );
        let id_3 = id_de(&con, ID);
        assert_eq!(filas(&con), 3);
        assert_eq!(textos(&con, id_3), ["tercera"]);

        let vers = lector::versiones(&con, id_3).unwrap();
        assert_eq!(
            vers.iter().map(|v| v.id).collect::<Vec<_>>(),
            vec![id_1, id_2]
        );
        assert!(vers.iter().all(|v| v.importada.ends_with('Z')));
        // La más antigua es `@1` y todas apuntan a la más reciente, no a la intermedia.
        assert_eq!(id_de(&con, &format!("{ID}@1")), id_1);
        assert_eq!(id_de(&con, &format!("{ID}@2")), id_2);
        assert_eq!(textos(&con, id_1), ["primera"]);
        assert_eq!(textos(&con, id_2), ["segunda"]);
        let sueltas: i64 = con
            .query_row(
                "SELECT count(*) FROM conversaciones WHERE version_de IS NOT ?1 AND id != ?1",
                [id_3],
                |f| f.get(0),
            )
            .unwrap();
        assert_eq!(sueltas, 0);
    }

    #[test]
    fn id_publico_resuelve_a_la_mas_reciente() {
        let (_d, mut con) = base();
        // Más versiones que el límite de filas de `resolver`.
        for n in 0..8 {
            importar(&mut con, &conv(vec![m(&format!("edición {n}"))]));
        }
        let publico = busqueda::id_publico(Fuente::ClaudeWeb, ID);
        assert_eq!(publico, "cw-aaaa1111");
        assert_eq!(
            busqueda::resolver(&con, &publico).unwrap(),
            Resolucion::Uno(id_de(&con, ID))
        );
    }

    #[test]
    fn el_id_de_una_version_no_se_presenta_como_excluida() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("primera")]));
        importar(&mut con, &conv(vec![m("segunda")]));
        assert_eq!(
            busqueda::resolver(&con, &format!("cw-{ID}@1")).unwrap(),
            Resolucion::Ninguno
        );
    }

    #[test]
    fn fts_no_devuelve_version_antigua() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("zanahoria")]));
        importar(&mut con, &conv(vec![m("remolacha")]));
        assert!(resultados(&con, "zanahoria").is_empty());
        assert_eq!(resultados(&con, "remolacha"), [ID]);
        // No se ha borrado: solo deja de listarse.
        let en_indice: i64 = con
            .query_row(
                "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH 'zanahoria'",
                [],
                |f| f.get(0),
            )
            .unwrap();
        assert_eq!(en_indice, 1);
    }

    #[test]
    fn recientes_lista_lo_web_sin_versiones() {
        let (_d, mut con) = base();
        importar(&mut con, &conv(vec![m("primera")]));
        importar(&mut con, &conv(vec![m("segunda")]));
        let r = busqueda::recientes(&con, &Filtros::default()).unwrap();
        assert_eq!(r.total_sesiones, 1);
        assert_eq!(r.resultados[0].id_original, ID);
        assert_eq!(r.resultados[0].fragmento, "segunda");
        let f = Filtros {
            fuente: Some(Fuente::ClaudeWeb),
            ..Filtros::default()
        };
        assert_eq!(busqueda::recientes(&con, &f).unwrap().total_sesiones, 1);
    }

    #[test]
    fn original_se_guarda_y_sobrevive_a_una_version() {
        let (_d, mut con) = base();
        let mut a = conv(vec![m("uno")]);
        a.original = Some("{\"v\":1}".to_string());
        importar(&mut con, &a);
        let id_viejo = id_de(&con, ID);
        assert_eq!(originales(&con, id_viejo), ["{\"v\":1}"]);

        let mut b = conv(vec![m("distinto")]);
        b.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &b), Efecto::Versionada);
        assert_eq!(originales(&con, id_viejo), ["{\"v\":1}"]);
        assert_eq!(originales(&con, id_de(&con, ID)), ["{\"v\":2}"]);
    }

    #[test]
    fn actualizar_anade_el_original_y_sin_original_no_lo_borra() {
        let (_d, mut con) = base();
        let mut a = conv(vec![m("uno")]);
        a.original = Some("{\"v\":1}".to_string());
        importar(&mut con, &a);
        let id = id_de(&con, ID);

        let mut b = conv(vec![m("uno"), m("dos")]);
        b.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &b), Efecto::Actualizada);
        assert_eq!(originales(&con, id), ["{\"v\":1}", "{\"v\":2}"]);

        let c = conv(vec![m("uno"), m("dos"), m("tres")]);
        assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
        assert_eq!(originales(&con, id), ["{\"v\":1}", "{\"v\":2}"]);
    }

    #[test]
    fn documento_se_busca_y_se_lee() {
        let (_d, mut con) = base();
        let mut c = conv(vec![m("Guía de compostaje doméstico")]);
        c.tipo = TipoWeb::Documento;
        c.proyecto = Some("Huerto".to_string());
        importar(&mut con, &c);
        let id = id_de(&con, ID);
        let tipo: String = con
            .query_row("SELECT tipo FROM conversaciones WHERE id = ?1", [id], |f| {
                f.get(0)
            })
            .unwrap();
        assert_eq!(tipo, "documento");
        assert_eq!(resultados(&con, "compostaje"), [ID]);
        let l = lector::leer(&con, id, &Tramo::default()).unwrap();
        assert_eq!(l.fuente, Fuente::ClaudeWeb);
        assert_eq!(
            l.bloques,
            [Bloque::Usuario {
                hora: None,
                texto: "Guía de compostaje doméstico".to_string()
            }]
        );
    }

    const T1: &str = "2026-10-01T10:00:00Z";
    const T2: &str = "2026-10-02T10:00:00Z";
    const T3: &str = "2026-10-03T10:00:00Z";
    const T4: &str = "2026-10-04T10:00:00Z";
    const T5: &str = "2026-10-05T10:00:00Z";

    /// Una exportación completa: mensajes, fecha y JSON propio.
    fn zip(mensajes: &[&str], fin: Option<&str>, json: &str) -> ConversacionImportada {
        let mut c = con_fin(mensajes.iter().map(|t| m(t)).collect(), fin);
        c.original = Some(json.to_string());
        c
    }

    fn titulado(mut c: ConversacionImportada, titulo: &str) -> ConversacionImportada {
        c.titulo = Some(titulo.to_string());
        c
    }

    fn titulo_de(con: &Connection, id: i64) -> String {
        con.query_row(
            "SELECT titulo FROM conversaciones WHERE id = ?1",
            [id],
            |f| f.get(0),
        )
        .unwrap()
    }

    fn proyecto_de(con: &Connection, id: i64) -> Option<String> {
        con.query_row(
            "SELECT proyecto FROM conversaciones WHERE id = ?1",
            [id],
            |f| f.get(0),
        )
        .unwrap()
    }

    #[test]
    fn regla_1_sin_actual_inserta_con_su_original() {
        let (_d, mut con) = base();
        assert_eq!(
            importar(&mut con, &zip(&["a"], Some(T1), "o1")),
            Efecto::Nueva
        );
        let id = id_de(&con, ID);
        assert_eq!(textos(&con, id), ["a"]);
        assert_eq!(fin_de(&con, id).as_deref(), Some(T1));
        assert_eq!(originales(&con, id), ["o1"]);
    }

    #[test]
    fn regla_2_viejo_solo_anade_el_original() {
        let (_d, mut con) = base();
        importar(&mut con, &titulado(zip(&["a"], Some(T3), "o3"), "Nuevo"));
        let id = id_de(&con, ID);
        let mut vieja = titulado(zip(&["a"], Some(T1), "o1"), "Viejo");
        vieja.proyecto = Some("Huerto".to_string());
        vieja.tipo = TipoWeb::Documento;
        assert_eq!(importar(&mut con, &vieja), Efecto::Igual);
        assert_eq!(titulo_de(&con, id), "Nuevo");
        assert_eq!(proyecto_de(&con, id), None);
        assert_eq!(fin_de(&con, id).as_deref(), Some(T3));
        assert_eq!(originales(&con, id), ["o3", "o1"]);
        assert_eq!(filas(&con), 1);
    }

    #[test]
    fn regla_2_sin_ser_viejo_aplica_metadatos_y_avanza_fin() {
        // Más nuevo, mismo instante y sin fecha: los tres aplican los metadatos.
        for (fin, esperado) in [(Some(T2), T2), (Some(T1), T1), (None, T1)] {
            let (_d, mut con) = base();
            importar(&mut con, &zip(&["a"], Some(T1), "o1"));
            let id = id_de(&con, ID);
            let mut c = titulado(zip(&["a"], fin, "o2"), "Otro");
            c.proyecto = Some("Huerto".to_string());
            assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
            assert_eq!(titulo_de(&con, id), "Otro");
            assert_eq!(proyecto_de(&con, id).as_deref(), Some("Huerto"));
            assert_eq!(fin_de(&con, id).as_deref(), Some(esperado));
            assert_eq!(originales(&con, id), ["o1", "o2"]);
        }
    }

    #[test]
    fn regla_2_sin_cambios_es_igual_aunque_avance_el_fin() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T1), "o1"));
        let id = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &zip(&["a"], Some(T2), "o2")),
            Efecto::Igual
        );
        assert_eq!(fin_de(&con, id).as_deref(), Some(T2));
        assert_eq!(originales(&con, id), ["o1", "o2"]);
    }

    #[test]
    fn regla_3_viejo_igual_a_una_version_anade_a_esa_version() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T1), "o1"));
        let vieja = id_de(&con, ID);
        importar(&mut con, &zip(&["z"], Some(T3), "o3"));
        let id = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &zip(&["a"], Some(T2), "o2")),
            Efecto::Igual
        );
        assert_eq!(originales(&con, vieja), ["o1", "o2"]);
        assert_eq!(originales(&con, id), ["o3"]);
        assert_eq!(fin_de(&con, id).as_deref(), Some(T3));
        assert_eq!(filas(&con), 2);
    }

    #[test]
    fn regla_3_viejo_distinto_se_archiva_sin_heredar() {
        let (_d, mut con) = base();
        let mut actual = zip(&["a"], Some(T3), "o3");
        actual.proyecto = Some("Huerto".to_string());
        importar(&mut con, &actual);
        let id = id_de(&con, ID);

        let mut vieja = zip(&["a", "b"], Some(T1), "o1");
        vieja.titulo = None;
        assert_eq!(importar(&mut con, &vieja), Efecto::Versionada);

        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["a"]);
        assert_eq!(originales(&con, id), ["o3"]);
        let version = lector::versiones(&con, id).unwrap()[0].id;
        assert_eq!(textos(&con, version), ["a", "b"]);
        assert_eq!(originales(&con, version), ["o1"]);
        assert_eq!(fin_de(&con, version).as_deref(), Some(T1));
        // Una versión vieja es como era entonces: no recibe los datos de la actual.
        assert_eq!(proyecto_de(&con, version), None);
        assert_ne!(titulo_de(&con, version), titulo_de(&con, id));
    }

    #[test]
    fn regla_4_nuevo_igual_a_una_version_vuelve_a_ser_la_actual() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T1), "o1"));
        let primera = id_de(&con, ID);
        let mut z = titulado(zip(&["z"], Some(T2), "o2"), "Zeta");
        z.proyecto = Some("Huerto".to_string());
        importar(&mut con, &z);
        let segunda = id_de(&con, ID);

        let mut vuelta = zip(&["a"], Some(T3), "o3");
        vuelta.titulo = None;
        assert_eq!(importar(&mut con, &vuelta), Efecto::Versionada);

        let id = id_de(&con, ID);
        assert_eq!(textos(&con, id), ["a"]);
        assert_eq!(fin_de(&con, id).as_deref(), Some(T3));
        assert_eq!(originales(&con, id), ["o3"]);
        // Lo que no llega se hereda de la actual que acaba de desplazar.
        assert_eq!(titulo_de(&con, id), "Zeta");
        assert_eq!(proyecto_de(&con, id).as_deref(), Some("Huerto"));
        let vers: Vec<i64> = lector::versiones(&con, id)
            .unwrap()
            .iter()
            .map(|v| v.id)
            .collect();
        assert_eq!(vers, [primera, segunda]);
        assert_eq!(originales(&con, primera), ["o1"]);
        assert_eq!(originales(&con, segunda), ["o2"]);
    }

    #[test]
    fn regla_4_sin_ser_nuevo_anade_a_esa_version() {
        for fin in [Some(T2), None] {
            let (_d, mut con) = base();
            importar(&mut con, &zip(&["a"], Some(T1), "o1"));
            let vieja = id_de(&con, ID);
            importar(&mut con, &zip(&["z"], Some(T2), "o2"));
            let id = id_de(&con, ID);
            assert_eq!(importar(&mut con, &zip(&["a"], fin, "o3")), Efecto::Igual);
            assert_eq!(id_de(&con, ID), id);
            assert_eq!(textos(&con, id), ["z"]);
            assert_eq!(originales(&con, vieja), ["o1", "o3"]);
            assert_eq!(originales(&con, id), ["o2"]);
        }
    }

    #[test]
    fn regla_4_una_actual_sin_fecha_recibe_el_fin() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], None, "o1"));
        importar(&mut con, &zip(&["z"], None, "o2"));
        let id = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &zip(&["a"], Some(T1), "o3")),
            Efecto::Igual
        );
        assert_eq!(textos(&con, id), ["z"]);
        assert_eq!(fin_de(&con, id).as_deref(), Some(T1));
    }

    #[test]
    fn regla_5_prefijo_reemplaza_y_conserva_los_originales() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T1), "o1"));
        let id = id_de(&con, ID);
        let c = titulado(zip(&["a", "b"], Some(T2), "o2"), "Otro");
        assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["a", "b"]);
        assert_eq!(titulo_de(&con, id), "Otro");
        assert_eq!(fin_de(&con, id).as_deref(), Some(T2));
        assert_eq!(originales(&con, id), ["o1", "o2"]);
        assert_eq!(filas(&con), 1);
    }

    #[test]
    fn regla_6_el_resto_versiona_y_hereda_lo_vacio() {
        let (_d, mut con) = base();
        let mut a = titulado(zip(&["a", "b"], Some(T1), "o1"), "Primero");
        a.proyecto = Some("Huerto".to_string());
        importar(&mut con, &a);
        let vieja = id_de(&con, ID);

        let mut c = zip(&["a", "c"], None, "o2");
        c.titulo = None;
        c.inicio = None;
        assert_eq!(importar(&mut con, &c), Efecto::Versionada);

        let id = id_de(&con, ID);
        assert_ne!(id, vieja);
        assert_eq!(textos(&con, id), ["a", "c"]);
        assert_eq!(titulo_de(&con, id), "Primero");
        assert_eq!(proyecto_de(&con, id).as_deref(), Some("Huerto"));
        assert_eq!(fin_de(&con, id).as_deref(), Some(T1));
        assert_eq!(originales(&con, id), ["o2"]);
        assert_eq!(originales(&con, vieja), ["o1"]);
    }

    #[test]
    fn invariante_a_el_mismo_json_no_se_repite_en_una_fila() {
        let (_d, mut con) = base();
        let c = zip(&["a"], Some(T1), "o1");
        importar(&mut con, &c);
        importar(&mut con, &c);
        assert_eq!(originales(&con, id_de(&con, ID)), ["o1"]);
        assert_eq!(total_originales(&con), 1);
    }

    #[test]
    fn invariante_b_un_fin_que_no_se_entiende_no_borra() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T1), "o1"));
        let casos = [
            (vec!["a"], Efecto::Igual),
            (vec!["a", "b"], Efecto::Actualizada),
            (vec!["otra"], Efecto::Versionada),
        ];
        for (mensajes, efecto) in casos {
            assert_eq!(
                importar(&mut con, &zip(&mensajes, Some("ayer"), "o2")),
                efecto
            );
            assert_eq!(fin_de(&con, id_de(&con, ID)).as_deref(), Some(T1));
        }
    }

    #[test]
    fn invariante_b_un_fin_valido_sustituye_al_que_no_se_entiende() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some("ayer"), "o1"));
        assert_eq!(
            importar(&mut con, &zip(&["a"], Some(T1), "o2")),
            Efecto::Igual
        );
        assert_eq!(fin_de(&con, id_de(&con, ID)).as_deref(), Some(T1));
    }

    #[test]
    fn invariante_b_el_fin_se_compara_por_instante() {
        // Las 09:30 en -01:00 son las 10:30 UTC: más tarde, aunque como texto vayan antes.
        let tarde = "2026-10-03T09:30:00-01:00";
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T3), "o1"));
        importar(&mut con, &zip(&["a"], Some(tarde), "o2"));
        assert_eq!(fin_de(&con, id_de(&con, ID)).as_deref(), Some(tarde));
    }

    #[test]
    fn viejo_nuevo_viejo_conserva_todos_los_originales() {
        let (_d, mut con) = base();
        let viejo = titulado(zip(&["a"], Some(T1), "o-viejo"), "Viejo");
        let nuevo = titulado(zip(&["a"], Some(T3), "o-nuevo"), "Nuevo");
        assert_eq!(importar(&mut con, &viejo), Efecto::Nueva);
        assert_eq!(importar(&mut con, &nuevo), Efecto::Actualizada);
        assert_eq!(importar(&mut con, &viejo), Efecto::Igual);
        let id = id_de(&con, ID);
        assert_eq!(titulo_de(&con, id), "Nuevo");
        assert_eq!(fin_de(&con, id).as_deref(), Some(T3));
        assert_eq!(originales(&con, id), ["o-viejo", "o-nuevo"]);
    }

    #[test]
    fn fin_intermedio_no_gana() {
        let (_d, mut con) = base();
        importar(&mut con, &titulado(zip(&["a"], Some(T1), "o1"), "De T1"));
        importar(&mut con, &titulado(zip(&["a"], Some(T3), "o3"), "De T3"));
        assert_eq!(
            importar(&mut con, &titulado(zip(&["a"], Some(T2), "o2"), "De T2")),
            Efecto::Igual
        );
        let id = id_de(&con, ID);
        assert_eq!(titulo_de(&con, id), "De T3");
        assert_eq!(fin_de(&con, id).as_deref(), Some(T3));
        assert_eq!(originales(&con, id), ["o1", "o3", "o2"]);
    }

    #[test]
    fn encoger_y_volver_conserva_la_intermedia() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a", "b"], Some(T1), "o1"));
        let encogida = titulado(zip(&["a"], Some(T2), "o2"), "Encogida");
        assert_eq!(importar(&mut con, &encogida), Efecto::Versionada);
        let intermedia = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &zip(&["a", "b"], Some(T3), "o3")),
            Efecto::Versionada
        );
        let id = id_de(&con, ID);
        assert_eq!(textos(&con, id), ["a", "b"]);
        assert!(lector::versiones(&con, id)
            .unwrap()
            .iter()
            .any(|v| v.id == intermedia));
        assert_eq!(textos(&con, intermedia), ["a"]);
        assert_eq!(originales(&con, intermedia), ["o2"]);
        assert_eq!(titulo_de(&con, intermedia), "Encogida");
    }

    #[test]
    fn crecer_con_fin_rancio_no_gana() {
        let (_d, mut con) = base();
        importar(&mut con, &zip(&["a"], Some(T1), "o1"));
        importar(&mut con, &zip(&["a"], Some(T3), "o3"));
        let id = id_de(&con, ID);
        assert_eq!(
            importar(&mut con, &zip(&["a", "b"], Some(T2), "o2")),
            Efecto::Versionada
        );
        assert_eq!(id_de(&con, ID), id);
        assert_eq!(textos(&con, id), ["a"]);
        assert_eq!(fin_de(&con, id).as_deref(), Some(T3));
        let version = lector::versiones(&con, id).unwrap()[0].id;
        assert_eq!(textos(&con, version), ["a", "b"]);
        assert_eq!(originales(&con, version), ["o2"]);
    }

    #[test]
    fn fin_nunca_retrocede() {
        use Efecto::{Actualizada, Igual, Nueva, Versionada};
        type Paso = (&'static [&'static str], Option<&'static str>, Efecto);
        // Una fila por rama de la tabla, con fechas viejas, ausentes y sin sentido entre medias.
        let pasos: [Paso; 15] = [
            (&["a"], Some(T2), Nueva),                 // 1
            (&["a"], Some(T1), Igual),                 // 2, viejo
            (&["a"], Some(T3), Actualizada),           // 2, otro título
            (&["a"], None, Igual),                     // 2, sin fecha
            (&["a"], Some("ayer"), Igual),             // 2, fecha sin sentido
            (&["a", "b"], Some(T2), Versionada),       // 3, se archiva
            (&["a", "b"], Some(T1), Igual),            // 3, ya era una versión
            (&["a", "b"], Some(T3), Igual),            // 4, sin ser nuevo
            (&["a", "b"], None, Igual),                // 4, sin fecha
            (&["a", "b"], Some(T4), Versionada),       // 4, nuevo
            (&["x", "y"], Some(T3), Versionada),       // 3, se archiva
            (&["a", "b", "c"], Some(T5), Actualizada), // 5
            (&["z"], None, Versionada),                // 6, sin fecha
            (&["y"], Some(T3), Versionada),            // 3, se archiva
            (&["w"], Some("ayer"), Versionada),        // 6, fecha sin sentido
        ];
        let instante = |con: &Connection| -> Option<f64> {
            con.query_row(
                "SELECT julianday(fin) FROM conversaciones WHERE version_de IS NULL",
                [],
                |f| f.get(0),
            )
            .unwrap()
        };
        let (_d, mut con) = base();
        let (mut fin_previo, mut originales_previos) = (None, 0);
        for (n, (mensajes, fin, efecto)) in pasos.into_iter().enumerate() {
            let mut c = zip(mensajes, fin, &format!("o{n}"));
            // Solo el tercer paso trae título: el resto se fija en las fechas.
            c.titulo = (n == 2).then(|| "Otro".to_string());
            assert_eq!(importar(&mut con, &c), efecto, "paso {n}");
            let (fin_ahora, originales_ahora) = (instante(&con), total_originales(&con));
            assert!(fin_ahora.is_some(), "paso {n}");
            assert!(fin_ahora >= fin_previo, "paso {n}");
            assert!(originales_ahora > originales_previos, "paso {n}");
            (fin_previo, originales_previos) = (fin_ahora, originales_ahora);
        }
    }

    #[test]
    fn ningun_mensaje_ni_original_se_pierde() {
        // Por orden de llegada, no cronológico: un archivo real son varios ZIP sueltos.
        let importaciones: [(&[&str], Option<&str>); 10] = [
            (&["a", "b"], Some(T3)),
            (&["a"], Some(T1)),
            (&["a", "b", "c"], Some(T4)),
            (&["a", "b"], Some(T2)),
            (&["otra"], Some(T5)),
            (&["a", "b", "c", "d"], Some(T4)),
            (&["otra", "más"], None),
            (&["x"], None),
            (&["a"], Some(T5)),
            (&["otra"], Some(T1)),
        ];
        let (_d, mut con) = base();
        for (n, (mensajes, fin)) in importaciones.iter().enumerate() {
            importar(&mut con, &zip(mensajes, *fin, &format!("o{n}")));
        }

        let mut q = con.prepare("SELECT id FROM conversaciones").unwrap();
        let ids: Vec<i64> = q
            .query_map([], |f| f.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let contenidos: Vec<Vec<String>> = ids.iter().map(|&i| textos(&con, i)).collect();
        let guardados: Vec<String> = ids.iter().flat_map(|&i| originales(&con, i)).collect();
        for (n, (mensajes, _)) in importaciones.iter().enumerate() {
            // Crecer deja la fila con más mensajes: el conjunto sigue entero dentro de ella.
            assert!(
                contenidos
                    .iter()
                    .any(|fila| mensajes.iter().all(|t| fila.iter().any(|x| x == t))),
                "se perdió {mensajes:?}"
            );
            assert!(guardados.contains(&format!("o{n}")), "se perdió o{n}");
        }
        assert_eq!(guardados.len(), importaciones.len());
    }
}
