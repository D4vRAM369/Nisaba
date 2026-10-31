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

/// La fila guardada con ese id, con lo que hace falta para comparar.
struct Previa {
    id: i64,
    huella: Option<String>,
    titulo: Option<String>,
    proyecto: Option<String>,
    tipo: String,
}

/// Aplica la tabla de «Reimportar» de la spec dentro de la transacción de quien llama:
/// la importación entera se confirma o se descarta de una vez.
pub fn fusionar(tx: &Transaction, c: &ConversacionImportada) -> rusqlite::Result<Efecto> {
    // `@` marca las versiones (`<id>@<n>`): un ZIP manipulado podría colarse en una de ellas.
    if c.base.id_original.contains('@') {
        let motivo = format!("id de conversación con @: {}", c.base.id_original);
        return Err(rusqlite::Error::ToSqlConversionFailure(motivo.into()));
    }
    let huella = huella_web(c);
    let previa = tx
        .query_row(
            "SELECT id, huella, titulo, proyecto, tipo FROM conversaciones
             WHERE fuente=?1 AND id_original=?2",
            params![c.base.fuente.as_str(), c.base.id_original],
            |f| {
                Ok(Previa {
                    id: f.get(0)?,
                    huella: f.get(1)?,
                    titulo: f.get(2)?,
                    proyecto: f.get(3)?,
                    tipo: f.get(4)?,
                })
            },
        )
        .optional()?;
    let Some(p) = previa else {
        escribir(tx, c, c.titulo.as_deref(), c.proyecto.as_deref(), &huella)?;
        return Ok(Efecto::Nueva);
    };
    if p.huella.as_deref() == Some(huella.as_str()) {
        if datos_distintos(&p, c) {
            actualizar_datos(tx, p.id, c)?;
            return Ok(Efecto::Actualizada);
        }
        rellenar_original(tx, p.id, c)?;
        return Ok(Efecto::Igual);
    }
    // Un ZIP antiguo trae lo que ya se guardó como versión: no debe volver a ser la vigente.
    let version: Option<i64> = tx
        .query_row(
            "SELECT id FROM conversaciones WHERE version_de = ?1 AND huella = ?2
             ORDER BY id LIMIT 1",
            params![p.id, huella],
            |f| f.get(0),
        )
        .optional()?;
    if let Some(version) = version {
        rellenar_original(tx, version, c)?;
        return Ok(Efecto::Igual);
    }
    let titulo = c.titulo.as_deref().or(p.titulo.as_deref());
    let proyecto = c.proyecto.as_deref().or(p.proyecto.as_deref());
    if solo_anade(&cargar_mensajes(tx, p.id)?, &c.base.mensajes) {
        escribir(tx, c, titulo, proyecto, &huella)?;
        return Ok(Efecto::Actualizada);
    }
    versionar(tx, p.id, c, titulo, proyecto, &huella)?;
    Ok(Efecto::Versionada)
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
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Un título o proyecto ausente no es un cambio: la exportación solo no lo trae.
fn datos_distintos(p: &Previa, c: &ConversacionImportada) -> bool {
    (c.titulo.is_some() && c.titulo != p.titulo)
        || (c.proyecto.is_some() && c.proyecto != p.proyecto)
        || c.tipo.as_str() != p.tipo
}

/// Mismo contenido con otros datos: no se tocan los mensajes ni `importada`, que fecha el contenido.
fn actualizar_datos(tx: &Transaction, id: i64, c: &ConversacionImportada) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE conversaciones SET titulo = COALESCE(?1, titulo),
           proyecto = COALESCE(?2, proyecto), tipo = ?3 WHERE id = ?4",
        params![c.titulo, c.proyecto, c.tipo.as_str(), id],
    )?;
    guardar_original(tx, id, c)
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

/// La fila vieja conserva su `id` (y con él mensajes y original); solo cambia de nombre para
/// dejar libre el id limpio. Con `@<n>` por orden de llegada: `@1` es siempre la más antigua.
fn versionar(
    tx: &Transaction,
    id_previo: i64,
    c: &ConversacionImportada,
    titulo: Option<&str>,
    proyecto: Option<&str>,
    huella: &str,
) -> rusqlite::Result<()> {
    let anteriores: i64 = tx.query_row(
        "SELECT count(*) FROM conversaciones WHERE version_de = ?1",
        [id_previo],
        |f| f.get(0),
    )?;
    tx.execute(
        "UPDATE conversaciones SET id_original = ?1 WHERE id = ?2",
        params![
            format!("{}@{}", c.base.id_original, anteriores + 1),
            id_previo
        ],
    )?;
    let id = escribir(tx, c, titulo, proyecto, huella)?;
    tx.execute(
        "UPDATE conversaciones SET version_de = ?1 WHERE id = ?2 OR version_de = ?2",
        params![id, id_previo],
    )?;
    Ok(())
}

/// Crea la fila con el id limpio o, si existe, la sustituye por la nueva. `titulo` y `proyecto`
/// ya vienen resueltos: los del ZIP o, si no los trae, los que había.
fn escribir(
    tx: &Transaction,
    c: &ConversacionImportada,
    titulo: Option<&str>,
    proyecto: Option<&str>,
    huella: &str,
) -> rusqlite::Result<i64> {
    let conv = &c.base;
    let titulo = titulo
        .map(str::to_string)
        .unwrap_or_else(|| titulo_util(proyecto, &conv.mensajes));
    let inicio = c
        .inicio
        .as_ref()
        .or_else(|| conv.mensajes.iter().find_map(|m| m.hora.as_ref()));
    let fin = c
        .fin
        .as_ref()
        .or_else(|| conv.mensajes.iter().rev().find_map(|m| m.hora.as_ref()));
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
            conv.id_original,
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
    guardar_original(tx, id, c)?;
    Ok(id)
}

/// Sin original no se borra el guardado: una importación que no lo trae no quita lo que había.
fn guardar_original(tx: &Transaction, id: i64, c: &ConversacionImportada) -> rusqlite::Result<()> {
    if let Some(json) = &c.original {
        tx.execute(
            "INSERT INTO originales(conversacion_id, json) VALUES (?1, ?2)
             ON CONFLICT(conversacion_id) DO UPDATE SET json = excluded.json",
            params![id, json],
        )?;
    }
    Ok(())
}

/// Para contenido `Igual`: guarda el original solo si faltaba.
fn rellenar_original(tx: &Transaction, id: i64, c: &ConversacionImportada) -> rusqlite::Result<()> {
    if let Some(json) = &c.original {
        tx.execute(
            "INSERT INTO originales(conversacion_id, json) VALUES (?1, ?2)
             ON CONFLICT(conversacion_id) DO NOTHING",
            params![id, json],
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

    fn original(con: &Connection, id: i64) -> Option<String> {
        con.query_row(
            "SELECT json FROM originales WHERE conversacion_id = ?1",
            [id],
            |f| f.get(0),
        )
        .ok()
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
        assert_eq!(original(&con, id).as_deref(), Some("{\"v\":2}"));
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
        assert_eq!(original(&con, id).as_deref(), Some("{\"v\":1}"));

        // Solo rellena: el que ya estaba no se pisa.
        con_original.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &con_original), Efecto::Igual);
        assert_eq!(original(&con, id).as_deref(), Some("{\"v\":1}"));
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
        assert_eq!(original(&con, vieja).as_deref(), Some("{\"v\":1}"));
        assert_eq!(original(&con, id_de(&con, ID)), None);
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
        assert_eq!(original(&con, id_viejo).as_deref(), Some("{\"v\":1}"));

        let mut b = conv(vec![m("distinto")]);
        b.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &b), Efecto::Versionada);
        assert_eq!(original(&con, id_viejo).as_deref(), Some("{\"v\":1}"));
        assert_eq!(
            original(&con, id_de(&con, ID)).as_deref(),
            Some("{\"v\":2}")
        );
    }

    #[test]
    fn actualizar_renueva_el_original_y_sin_original_no_lo_borra() {
        let (_d, mut con) = base();
        let mut a = conv(vec![m("uno")]);
        a.original = Some("{\"v\":1}".to_string());
        importar(&mut con, &a);
        let id = id_de(&con, ID);

        let mut b = conv(vec![m("uno"), m("dos")]);
        b.original = Some("{\"v\":2}".to_string());
        assert_eq!(importar(&mut con, &b), Efecto::Actualizada);
        assert_eq!(original(&con, id).as_deref(), Some("{\"v\":2}"));

        let c = conv(vec![m("uno"), m("dos"), m("tres")]);
        assert_eq!(importar(&mut con, &c), Efecto::Actualizada);
        assert_eq!(original(&con, id).as_deref(), Some("{\"v\":2}"));
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
}
