//! Lector: convierte una conversación indexada en bloques de lectura y Markdown.

use crate::busqueda::id_publico;
use crate::modelo::Fuente;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct Tramo {
    pub desde: Option<usize>,
    pub cantidad: usize,
    /// `orden` de mensaje alrededor del cual centrar el tramo.
    pub alrededor_de: Option<i64>,
    /// Muestra todo: ocultos, sistema y cada parte como bloque propio.
    pub todo: bool,
}

impl Default for Tramo {
    fn default() -> Self {
        Tramo {
            desde: None,
            cantidad: 20,
            alrededor_de: None,
            todo: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tipo")]
pub enum Bloque {
    Usuario {
        hora: Option<String>,
        texto: String,
    },
    Asistente {
        modelo: Option<String>,
        hora: Option<String>,
        texto: String,
    },
    Herramientas {
        n: usize,
        archivos: usize,
    },
    Razonamiento {
        texto: String,
        desplegado: bool,
    },
    Herramienta {
        texto: String,
    },
    Subagente {
        id_publico: String,
        descripcion: String,
    },
    Sistema {
        texto: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Lectura {
    pub titulo: String,
    pub id_publico: String,
    pub total: usize,
    pub desde: usize,
    pub bloques: Vec<Bloque>,
    /// Índice absoluto del bloque que corresponde a alrededor_de, si se pidió.
    pub coincidencia: Option<usize>,
    pub cwd: Option<String>,
    pub id_original: String,
    pub fuente: Fuente,
    /// Tiene sesión madre: no se puede reanudar por sí sola.
    pub es_subagente: bool,
}

/// Una versión anterior de una conversación web.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Version {
    pub id: i64,
    /// Cuándo entró este contenido en el archivo (ISO, UTC).
    pub importada: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdjuntoWeb {
    pub nombre: String,
    pub tipo: Option<String>,
    pub tamano: Option<u64>,
    pub texto_extraido: Option<String>,
    /// `true` cuando la exportación (p. ej. Claude.ai) no incluye el archivo original.
    pub sin_binario: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CitaWeb {
    pub numero: usize,
    pub url: String,
    pub tipo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HerramientaWeb {
    pub nombre: String,
    pub entrada: String,
    pub resultado: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentoProyecto {
    pub id_publico: String,
    pub titulo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RamaWeb {
    pub hoja: String,
    pub hora: Option<String>,
    pub mensajes: usize,
    pub activa: bool,
}

/// Datos adicionales de una conversación del archivo web para el panel lateral `Detalles`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DetallesWeb {
    pub tipo: String,
    pub proyecto: Option<String>,
    pub documentos_proyecto: Vec<DocumentoProyecto>,
    pub adjuntos: Vec<AdjuntoWeb>,
    pub citas: Vec<CitaWeb>,
    pub herramientas: Vec<HerramientaWeb>,
    pub versiones: Vec<Version>,
    pub ramas: Vec<RamaWeb>,
    pub html: Option<String>,
    pub tiene_original: bool,
}

pub(crate) struct Msg {
    orden: i64,
    pub(crate) uuid: Option<String>,
    pub(crate) id_padre: Option<String>,
    pub(crate) rol: String,
    pub(crate) modelo: Option<String>,
    pub(crate) hora: Option<String>,
    pub(crate) oculto: bool,
    /// `(tipo, contenido)`.
    pub(crate) partes: Vec<(String, String)>,
}

struct Hijo {
    id_publico: String,
    descripcion: String,
    usado: bool,
}

/// Conserva solo el camino desde la hoja hacia atrás. Sin `uuid`: todos.
/// Supuesto: Claude siempre trae uuid y Codex nunca, así que si alguno lo tiene
/// los mensajes sin uuid se descartan.
fn rama_activa(msgs: Vec<Msg>, hoja: Option<&str>) -> Vec<Msg> {
    if !msgs.iter().any(|m| m.uuid.is_some()) {
        return msgs;
    }
    let por_uuid: HashMap<&str, usize> = msgs
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.uuid.as_deref().map(|u| (u, i)))
        .collect();
    let inicio = hoja
        .and_then(|h| por_uuid.get(h).copied())
        .or_else(|| msgs.iter().rposition(|m| m.uuid.is_some()));
    let mut camino: HashSet<usize> = HashSet::new();
    let mut actual = inicio;
    while let Some(i) = actual {
        if !camino.insert(i) {
            break; // ciclo
        }
        actual = msgs[i]
            .id_padre
            .as_deref()
            .and_then(|p| por_uuid.get(p).copied());
        if actual.is_none() && msgs[i].id_padre.is_some() {
            // Padre ausente (cadena rota): no se trunca; se añaden los mensajes
            // con uuid que van antes en `orden`.
            camino.extend((0..i).filter(|&j| msgs[j].uuid.is_some()));
        }
    }
    msgs.into_iter()
        .enumerate()
        .filter(|(i, _)| camino.contains(i))
        .map(|(_, m)| m)
        .collect()
}

fn nombre_y_entrada(contenido: &str) -> Option<(&str, Value)> {
    let (nombre, resto) = contenido.split_once(": ")?;
    if nombre.is_empty() || nombre.contains(char::is_whitespace) {
        return None;
    }
    Some((nombre, serde_json::from_str(resto).ok()?))
}

/// Acumula herramientas consecutivas hasta que algo las interrumpe.
#[derive(Default)]
struct Grupo {
    n: usize,
    archivos: HashSet<String>,
}

impl Grupo {
    fn sumar(&mut self, contenido: &str) {
        self.n += 1;
        if let Some((nombre, v)) = nombre_y_entrada(contenido) {
            if matches!(nombre, "Read" | "Edit" | "Write") {
                if let Some(f) = v.get("file_path").and_then(Value::as_str) {
                    self.archivos.insert(f.to_string());
                }
            }
        }
    }
}

struct Constructor {
    bloques: Vec<(i64, Bloque)>,
    grupo: Option<(i64, Grupo)>,
}

impl Constructor {
    fn cerrar(&mut self) {
        if let Some((orden, g)) = self.grupo.take() {
            self.bloques.push((
                orden,
                Bloque::Herramientas {
                    n: g.n,
                    archivos: g.archivos.len(),
                },
            ));
        }
    }
    fn poner(&mut self, orden: i64, b: Bloque) {
        self.cerrar();
        self.bloques.push((orden, b));
    }
}

fn bloque_texto(m: &Msg, texto: String) -> Bloque {
    match m.rol.as_str() {
        "asistente" => Bloque::Asistente {
            modelo: m.modelo.clone(),
            hora: m.hora.clone(),
            texto,
        },
        "sistema" => Bloque::Sistema { texto },
        _ => Bloque::Usuario {
            hora: m.hora.clone(),
            texto,
        },
    }
}

pub(crate) fn cargar_mensajes(
    conn: &Connection,
    conversacion_id: i64,
) -> rusqlite::Result<Vec<Msg>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.orden, m.uuid, m.id_padre, m.rol, m.modelo, m.hora, m.oculto
         FROM mensajes m WHERE m.conversacion_id = ?1 ORDER BY m.orden",
    )?;
    let mut ps =
        conn.prepare("SELECT tipo, contenido FROM partes WHERE mensaje_id = ?1 ORDER BY orden")?;
    let filas = stmt.query_map([conversacion_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Msg {
                orden: r.get(1)?,
                uuid: r.get(2)?,
                id_padre: r.get(3)?,
                rol: r.get(4)?,
                modelo: r.get(5)?,
                hora: r.get(6)?,
                oculto: r.get::<_, i64>(7)? != 0,
                partes: vec![],
            },
        ))
    })?;
    let mut msgs = Vec::new();
    for f in filas {
        let (id, mut m) = f?;
        m.partes = ps
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        msgs.push(m);
    }
    Ok(msgs)
}

fn cargar_hijos(conn: &Connection, conversacion_id: i64) -> rusqlite::Result<Vec<Hijo>> {
    let mut stmt = conn.prepare(
        "SELECT fuente, id_original, COALESCE(descripcion, '') FROM conversaciones_visibles
         WHERE padre_id = ?1 ORDER BY inicio, id",
    )?;
    let filas = stmt.query_map([conversacion_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut hijos = Vec::new();
    for f in filas {
        let (fuente, id_original, descripcion) = f?;
        let Some(fuente) = Fuente::desde_str(&fuente) else {
            continue;
        };
        hijos.push(Hijo {
            id_publico: id_publico(fuente, &id_original),
            descripcion,
            usado: false,
        });
    }
    Ok(hijos)
}

pub fn leer(conn: &Connection, conversacion_id: i64, t: &Tramo) -> rusqlite::Result<Lectura> {
    leer_de(
        conn,
        "conversaciones_visibles WHERE id = ?1",
        conversacion_id,
        None,
        t,
    )
}

/// Lee una rama concreta (por uuid de su hoja) de una conversación visible.
pub fn leer_rama(
    conn: &Connection,
    conversacion_id: i64,
    hoja: &str,
    t: &Tramo,
) -> rusqlite::Result<Lectura> {
    leer_de(
        conn,
        "conversaciones_visibles WHERE id = ?1",
        conversacion_id,
        Some(hoja),
        t,
    )
}

/// Lee una versión anterior, que `leer` no ve porque no es visible. Cualquier otra fila es un
/// error. Solo en la tablilla web: la local no tiene `version_de`.
pub fn leer_version(conn: &Connection, version_id: i64, t: &Tramo) -> rusqlite::Result<Lectura> {
    leer_de(
        conn,
        "conversaciones WHERE id = ?1 AND version_de IS NOT NULL",
        version_id,
        None,
        t,
    )
}

/// `origen` es SQL fijo de este módulo: tabla y condición de la fila que se puede leer.
fn leer_de(
    conn: &Connection,
    origen: &str,
    conversacion_id: i64,
    hoja_forzada: Option<&str>,
    t: &Tramo,
) -> rusqlite::Result<Lectura> {
    #[allow(clippy::type_complexity)]
    let (fuente, id_original, titulo, hoja, cwd, es_subagente): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
    ) = conn.query_row(
        &format!(
            "SELECT fuente, id_original, titulo, hoja, cwd, padre_id IS NOT NULL FROM {origen}"
        ),
        [conversacion_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )?;
    let fuente = Fuente::desde_str(&fuente).ok_or(rusqlite::Error::InvalidQuery)?;
    let publico = id_publico(fuente, &id_original);
    let hoja_efectiva = hoja_forzada.or(hoja.as_deref());
    let msgs = rama_activa(cargar_mensajes(conn, conversacion_id)?, hoja_efectiva);
    let mut hijos = cargar_hijos(conn, conversacion_id)?;

    let mut c = Constructor {
        bloques: vec![],
        grupo: None,
    };
    for m in &msgs {
        let mut texto_pendiente: Option<String> = None;
        let vaciar = |c: &mut Constructor, pend: &mut Option<String>| {
            if let Some(texto) = pend.take() {
                c.poner(m.orden, bloque_texto(m, texto));
            }
        };
        for (tipo, contenido) in &m.partes {
            if t.todo {
                let b = if m.oculto || tipo == "sistema" {
                    Bloque::Sistema {
                        texto: contenido.clone(),
                    }
                } else {
                    match tipo.as_str() {
                        "razonamiento" => Bloque::Razonamiento {
                            texto: contenido.clone(),
                            desplegado: true,
                        },
                        "herramienta" => Bloque::Herramienta {
                            texto: contenido.clone(),
                        },
                        _ => bloque_texto(m, contenido.clone()),
                    }
                };
                c.poner(m.orden, b);
                continue;
            }
            if m.oculto || tipo == "sistema" {
                continue;
            }
            match tipo.as_str() {
                "herramienta" => {
                    vaciar(&mut c, &mut texto_pendiente);
                    c.grupo
                        .get_or_insert_with(|| (m.orden, Grupo::default()))
                        .1
                        .sumar(contenido);
                    if let Some((nombre, v)) = nombre_y_entrada(contenido) {
                        if matches!(nombre, "Agent" | "Task") {
                            let desc = v.get("description").and_then(Value::as_str).unwrap_or("");
                            let pos = hijos
                                .iter()
                                .position(|h| !h.usado && h.descripcion == desc)
                                .or_else(|| hijos.iter().position(|h| !h.usado));
                            if let Some(i) = pos {
                                hijos[i].usado = true;
                                c.cerrar();
                                c.bloques.push((
                                    m.orden,
                                    Bloque::Subagente {
                                        id_publico: hijos[i].id_publico.clone(),
                                        descripcion: hijos[i].descripcion.clone(),
                                    },
                                ));
                            }
                        }
                    }
                }
                "razonamiento" => {
                    vaciar(&mut c, &mut texto_pendiente);
                    c.poner(
                        m.orden,
                        Bloque::Razonamiento {
                            texto: contenido.clone(),
                            desplegado: false,
                        },
                    );
                }
                _ => {
                    // texto e imagen: texto consecutivo del mismo mensaje se une.
                    match &mut texto_pendiente {
                        Some(p) => {
                            p.push_str("\n\n");
                            p.push_str(contenido);
                        }
                        None => texto_pendiente = Some(contenido.clone()),
                    }
                }
            }
        }
        vaciar(&mut c, &mut texto_pendiente);
    }
    c.cerrar();
    // Subagentes que no se pudieron ubicar: al final.
    if !t.todo {
        let ultimo = c.bloques.last().map(|b| b.0).unwrap_or(0);
        for h in hijos.iter().filter(|h| !h.usado) {
            c.bloques.push((
                ultimo,
                Bloque::Subagente {
                    id_publico: h.id_publico.clone(),
                    descripcion: h.descripcion.clone(),
                },
            ));
        }
    }

    let total = c.bloques.len();
    let cantidad = t.cantidad.max(1);
    let coincidencia = t.alrededor_de.and_then(|o| {
        if total == 0 {
            None
        } else {
            Some(
                c.bloques
                    .iter()
                    .position(|(orden, _)| *orden >= o)
                    .unwrap_or(total - 1),
            )
        }
    });
    let desde = coincidencia
        .map(|pos| pos.saturating_sub(cantidad / 2))
        .unwrap_or_else(|| t.desde.unwrap_or(0))
        .min(total);
    let bloques = c
        .bloques
        .into_iter()
        .skip(desde)
        .take(cantidad)
        .map(|(_, b)| b)
        .collect();
    Ok(Lectura {
        titulo: titulo.unwrap_or_default(),
        id_publico: publico,
        total,
        desde,
        bloques,
        coincidencia,
        cwd,
        id_original,
        fuente,
        es_subagente,
    })
}

/// Versiones anteriores de una conversación web, por orden de llegada (`importada`).
/// Solo en la tablilla web: la local no tiene `version_de`.
pub fn versiones(conn: &Connection, conversacion_id: i64) -> rusqlite::Result<Vec<Version>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(importada, '') FROM conversaciones WHERE version_de = ?1 ORDER BY id",
    )?;
    let filas = stmt.query_map([conversacion_id], |r| {
        Ok(Version {
            id: r.get(0)?,
            importada: r.get(1)?,
        })
    })?;
    filas.collect()
}

/// Extrae para el panel lateral `Detalles` los adjuntos, citas, herramientas, documentos del
/// proyecto, versiones y ramas de una conversación en la tablilla web.
pub fn detalles_web(conn: &Connection, conversacion_id: i64) -> rusqlite::Result<DetallesWeb> {
    use rusqlite::OptionalExtension;
    #[allow(clippy::type_complexity)]
    let (proyecto, hoja, tipo, version_de): (
        Option<String>,
        Option<String>,
        String,
        Option<i64>,
    ) = conn.query_row(
        "SELECT proyecto, hoja, COALESCE(tipo, 'conversacion'), version_de
         FROM conversaciones WHERE id = ?1",
        [conversacion_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;

    let id_vigente = version_de.unwrap_or(conversacion_id);
    let vers = versiones(conn, id_vigente)?;

    let mut documentos_proyecto = Vec::new();
    if let Some(ref proy) = proyecto {
        if !proy.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT fuente, id_original, COALESCE(titulo, id_original)
                 FROM conversaciones_visibles
                 WHERE proyecto = ?1 AND tipo = 'documento' AND id != ?2
                 ORDER BY titulo COLLATE NOCASE, id",
            )?;
            let filas = stmt.query_map(rusqlite::params![proy, id_vigente], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for f in filas {
                let (f_str, id_orig, tit) = f?;
                if let Some(fuente) = Fuente::desde_str(&f_str) {
                    documentos_proyecto.push(DocumentoProyecto {
                        id_publico: id_publico(fuente, &id_orig),
                        titulo: tit,
                    });
                }
            }
        }
    }

    let msgs = cargar_mensajes(conn, conversacion_id)?;
    let ramas = calcular_ramas(&msgs, hoja.as_deref());
    let mut herramientas = extraer_herramientas(&msgs);
    let mut adjuntos = Vec::new();
    let mut citas = Vec::new();
    let mut html = None;

    let original: Option<String> = conn
        .query_row(
            "SELECT json FROM originales WHERE conversacion_id = ?1 ORDER BY rowid DESC LIMIT 1",
            [conversacion_id],
            |r| r.get(0),
        )
        .optional()?;
    let tiene_original = original.is_some();

    if let Some(ref raw) = original {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            extraer_de_original(&v, &mut adjuntos, &mut citas, &mut herramientas, &mut html);
        }
    }
    if html.is_none() && tipo == "documento" {
        for m in &msgs {
            for (tp, cont) in &m.partes {
                let rec = cont.trim_start();
                if tp == "texto"
                    && (rec.len() >= 14 && rec[..14].eq_ignore_ascii_case("<!doctype html")
                        || rec.len() >= 5 && rec[..5].eq_ignore_ascii_case("<html"))
                {
                    html = Some(cont.clone());
                    break;
                }
            }
        }
    }

    Ok(DetallesWeb {
        tipo,
        proyecto,
        documentos_proyecto,
        adjuntos,
        citas,
        herramientas,
        versiones: vers,
        ramas,
        html,
        tiene_original,
    })
}

fn calcular_ramas(msgs: &[Msg], hoja_activa: Option<&str>) -> Vec<RamaWeb> {
    if !msgs.iter().any(|m| m.uuid.is_some()) {
        return Vec::new();
    }
    let padres: HashSet<&str> = msgs.iter().filter_map(|m| m.id_padre.as_deref()).collect();
    let por_uuid: HashMap<&str, usize> = msgs
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.uuid.as_deref().map(|u| (u, i)))
        .collect();
    let activa_efectiva = hoja_activa.or_else(|| msgs.iter().rev().find_map(|m| m.uuid.as_deref()));

    let mut hojas = Vec::new();
    for (idx, m) in msgs.iter().enumerate() {
        let Some(u) = m.uuid.as_deref() else { continue };
        if !padres.contains(u) {
            let mut n = 0usize;
            let mut visitados = HashSet::new();
            let mut actual = Some(idx);
            while let Some(i) = actual {
                if !visitados.insert(i) {
                    break;
                }
                n += 1;
                actual = msgs[i]
                    .id_padre
                    .as_deref()
                    .and_then(|p| por_uuid.get(p).copied());
            }
            hojas.push(RamaWeb {
                hoja: u.to_string(),
                hora: m.hora.clone(),
                mensajes: n,
                activa: activa_efectiva == Some(u),
            });
        }
    }
    hojas
}

fn extraer_herramientas(msgs: &[Msg]) -> Vec<HerramientaWeb> {
    let mut res = Vec::new();
    for m in msgs {
        for (tp, cont) in &m.partes {
            if tp != "herramienta" {
                continue;
            }
            let (cabecera, resultado) = match cont.split_once("\nResultado: ") {
                Some((c, r)) => (c, Some(r.to_string())),
                None => (cont.as_str(), None),
            };
            let (nombre, entrada) = match cabecera.split_once(": ") {
                Some((n, e)) if !n.is_empty() && !n.contains(char::is_whitespace) => {
                    (n.to_string(), e.to_string())
                }
                _ => ("herramienta".to_string(), cabecera.to_string()),
            };
            res.push(HerramientaWeb {
                nombre,
                entrada,
                resultado,
            });
        }
    }
    res
}

fn extraer_de_original(
    v: &Value,
    adjuntos: &mut Vec<AdjuntoWeb>,
    citas: &mut Vec<CitaWeb>,
    _herramientas: &mut Vec<HerramientaWeb>,
    html: &mut Option<String>,
) {
    if let Some(h) = v.get("html").and_then(Value::as_str) {
        if !h.is_empty() {
            *html = Some(h.to_string());
        }
    }
    let mut urls_vistas: HashSet<String> = HashSet::new();
    if let Some(mensajes) = v.get("chat_messages").and_then(Value::as_array) {
        for m in mensajes {
            let mut nombres_msg = HashSet::new();
            if let Some(atts) = m.get("attachments").and_then(Value::as_array) {
                for a in atts {
                    let nombre = a
                        .get("file_name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("adjunto")
                        .to_string();
                    nombres_msg.insert(nombre.clone());
                    let texto_extraido = a
                        .get("extracted_content")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    adjuntos.push(AdjuntoWeb {
                        nombre,
                        tipo: a
                            .get("file_type")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                        tamano: a.get("file_size").and_then(Value::as_u64),
                        texto_extraido,
                        sin_binario: true,
                    });
                }
            }
            if let Some(files) = m.get("files").and_then(Value::as_array) {
                for f in files {
                    let nombre = f
                        .get("file_name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("archivo")
                        .to_string();
                    if nombres_msg.insert(nombre.clone()) {
                        adjuntos.push(AdjuntoWeb {
                            nombre,
                            tipo: None,
                            tamano: None,
                            texto_extraido: None,
                            sin_binario: true,
                        });
                    }
                }
            }
            if let Some(partes) = m.get("content").and_then(Value::as_array) {
                for p in partes {
                    if let Some(cs) = p.get("citations").and_then(Value::as_array) {
                        for c in cs {
                            let det = c.get("details");
                            if let Some(url) = det
                                .and_then(|d| d.get("url"))
                                .and_then(Value::as_str)
                                .or_else(|| c.get("url").and_then(Value::as_str))
                                .filter(|u| !u.is_empty())
                            {
                                if urls_vistas.insert(url.to_string()) {
                                    citas.push(CitaWeb {
                                        numero: citas.len() + 1,
                                        url: url.to_string(),
                                        tipo: det
                                            .and_then(|d| d.get("type"))
                                            .and_then(Value::as_str)
                                            .map(String::from),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(mensajes) = v.get("messages").and_then(Value::as_array) {
        for m in mensajes {
            if let Some(atts) = m
                .get("content")
                .and_then(|c| c.get("attachments"))
                .and_then(Value::as_array)
            {
                for a in atts {
                    let nombre = a
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("adjunto")
                        .to_string();
                    adjuntos.push(AdjuntoWeb {
                        nombre,
                        tipo: a.get("type").and_then(Value::as_str).map(String::from),
                        tamano: None,
                        texto_extraido: a
                            .get("content")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                        sin_binario: true,
                    });
                }
            }
        }
    }
}

fn con_hora(base: String, hora: &Option<String>) -> String {
    match hora {
        Some(h) if !h.is_empty() => format!("{base} · {h}"),
        _ => base,
    }
}

pub fn a_markdown(l: &Lectura) -> String {
    let fin = l.desde + l.bloques.len();
    let carpeta = l
        .cwd
        .as_deref()
        .map(|c| format!("📁 `{c}`\n\n"))
        .unwrap_or_default();
    let mut s = format!(
        "# {}\n\n`{}` · `{}` · bloques {}–{} de {}\n\n{carpeta}",
        l.titulo,
        l.id_publico,
        l.id_original,
        if l.bloques.is_empty() {
            l.desde
        } else {
            l.desde + 1
        },
        fin,
        l.total
    );
    s.push_str(&bloques_a_markdown(&l.bloques));
    if fin < l.total {
        s.push_str(&format!(
            "… siguiente tramo: nisaba ver {} --desde {fin}\n",
            l.id_publico
        ));
    }
    s
}

/// Solo el cuerpo: un apartado por bloque, sin cabecera ni aviso de tramo.
pub fn bloques_a_markdown(bloques: &[Bloque]) -> String {
    let mut s = String::new();
    for b in bloques {
        match b {
            Bloque::Usuario { hora, texto } => {
                s.push_str(&format!(
                    "{}\n\n{texto}\n\n",
                    con_hora("### 👤 Tú".into(), hora)
                ));
            }
            Bloque::Asistente {
                modelo,
                hora,
                texto,
            } => {
                let nombre = modelo
                    .as_deref()
                    .filter(|m| !m.is_empty())
                    .unwrap_or("Asistente");
                s.push_str(&format!(
                    "{}\n\n{texto}\n\n",
                    con_hora(format!("### 🤖 {nombre}"), hora)
                ));
            }
            Bloque::Herramientas { n, archivos } => {
                s.push_str(&format!("> 🔧 {n} herramientas · {archivos} archivos\n\n"));
            }
            Bloque::Razonamiento { texto, desplegado } => {
                if *desplegado {
                    s.push_str(&format!("### 💭 Razonamiento\n\n{texto}\n\n"));
                } else {
                    s.push_str(&format!(
                        "> 💭 Razonamiento ({} caracteres)\n\n",
                        texto.chars().count()
                    ));
                }
            }
            Bloque::Herramienta { texto } => {
                s.push_str(&format!("### 🔧 Herramienta\n\n{texto}\n\n"));
            }
            Bloque::Subagente {
                id_publico,
                descripcion,
            } => {
                s.push_str(&format!("> 🤖 Subagente {id_publico}: {descripcion}\n\n"));
            }
            Bloque::Sistema { texto } => {
                s.push_str(&format!(
                    "> ⚙️ Sistema: {}\n\n",
                    texto.replace('\n', "\n> ")
                ));
            }
        }
    }
    s
}
