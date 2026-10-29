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
        Tramo { desde: None, cantidad: 20, alrededor_de: None, todo: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tipo")]
pub enum Bloque {
    Usuario { hora: Option<String>, texto: String },
    Asistente { modelo: Option<String>, hora: Option<String>, texto: String },
    Herramientas { n: usize, archivos: usize },
    Razonamiento { texto: String, desplegado: bool },
    Herramienta { texto: String },
    Subagente { id_publico: String, descripcion: String },
    Sistema { texto: String },
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
}

struct Msg {
    orden: i64,
    uuid: Option<String>,
    id_padre: Option<String>,
    rol: String,
    modelo: Option<String>,
    hora: Option<String>,
    oculto: bool,
    partes: Vec<(String, String)>,
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
        actual = msgs[i].id_padre.as_deref().and_then(|p| por_uuid.get(p).copied());
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
            self.bloques.push((orden, Bloque::Herramientas { n: g.n, archivos: g.archivos.len() }));
        }
    }
    fn poner(&mut self, orden: i64, b: Bloque) {
        self.cerrar();
        self.bloques.push((orden, b));
    }
}

fn bloque_texto(m: &Msg, texto: String) -> Bloque {
    match m.rol.as_str() {
        "asistente" => Bloque::Asistente { modelo: m.modelo.clone(), hora: m.hora.clone(), texto },
        "sistema" => Bloque::Sistema { texto },
        _ => Bloque::Usuario { hora: m.hora.clone(), texto },
    }
}

fn cargar_mensajes(conn: &Connection, conversacion_id: i64) -> rusqlite::Result<Vec<Msg>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.orden, m.uuid, m.id_padre, m.rol, m.modelo, m.hora, m.oculto
         FROM mensajes m WHERE m.conversacion_id = ?1 ORDER BY m.orden",
    )?;
    let mut ps = conn.prepare("SELECT tipo, contenido FROM partes WHERE mensaje_id = ?1 ORDER BY orden")?;
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
        "SELECT fuente, id_original, COALESCE(descripcion, '') FROM conversaciones
         WHERE padre_id = ?1 ORDER BY inicio, id",
    )?;
    let filas = stmt.query_map([conversacion_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut hijos = Vec::new();
    for f in filas {
        let (fuente, id_original, descripcion) = f?;
        let Some(fuente) = Fuente::desde_str(&fuente) else { continue };
        hijos.push(Hijo { id_publico: id_publico(fuente, &id_original), descripcion, usado: false });
    }
    Ok(hijos)
}

pub fn leer(conn: &Connection, conversacion_id: i64, t: &Tramo) -> rusqlite::Result<Lectura> {
    let (fuente, id_original, titulo, hoja): (String, String, Option<String>, Option<String>) = conn.query_row(
        "SELECT fuente, id_original, titulo, hoja FROM conversaciones WHERE id = ?1",
        [conversacion_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let publico = match Fuente::desde_str(&fuente) {
        Some(f) => id_publico(f, &id_original),
        None => id_original.chars().take(8).collect(),
    };
    let msgs = rama_activa(cargar_mensajes(conn, conversacion_id)?, hoja.as_deref());
    let mut hijos = cargar_hijos(conn, conversacion_id)?;

    let mut c = Constructor { bloques: vec![], grupo: None };
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
                    Bloque::Sistema { texto: contenido.clone() }
                } else {
                    match tipo.as_str() {
                        "razonamiento" => Bloque::Razonamiento { texto: contenido.clone(), desplegado: true },
                        "herramienta" => Bloque::Herramienta { texto: contenido.clone() },
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
                    c.grupo.get_or_insert_with(|| (m.orden, Grupo::default())).1.sumar(contenido);
                    if let Some((nombre, v)) = nombre_y_entrada(contenido) {
                        if matches!(nombre, "Agent" | "Task") {
                            let desc = v.get("description").and_then(Value::as_str).unwrap_or("");
                            let pos = hijos.iter().position(|h| !h.usado && h.descripcion == desc)
                                .or_else(|| hijos.iter().position(|h| !h.usado));
                            if let Some(i) = pos {
                                hijos[i].usado = true;
                                c.cerrar();
                                c.bloques.push((m.orden, Bloque::Subagente {
                                    id_publico: hijos[i].id_publico.clone(),
                                    descripcion: hijos[i].descripcion.clone(),
                                }));
                            }
                        }
                    }
                }
                "razonamiento" => {
                    vaciar(&mut c, &mut texto_pendiente);
                    c.poner(m.orden, Bloque::Razonamiento { texto: contenido.clone(), desplegado: false });
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
            c.bloques.push((ultimo, Bloque::Subagente {
                id_publico: h.id_publico.clone(),
                descripcion: h.descripcion.clone(),
            }));
        }
    }

    let total = c.bloques.len();
    let cantidad = t.cantidad.max(1);
    let coincidencia = t.alrededor_de.and_then(|o| {
        if total == 0 { None } else {
            Some(c.bloques.iter().position(|(orden, _)| *orden >= o).unwrap_or(total - 1))
        }
    });
    let desde = coincidencia.map(|pos| pos.saturating_sub(cantidad / 2))
        .unwrap_or_else(|| t.desde.unwrap_or(0)).min(total);
    let bloques = c.bloques.into_iter().skip(desde).take(cantidad).map(|(_, b)| b).collect();
    Ok(Lectura { titulo: titulo.unwrap_or_default(), id_publico: publico, total, desde, bloques, coincidencia })
}

fn con_hora(base: String, hora: &Option<String>) -> String {
    match hora {
        Some(h) if !h.is_empty() => format!("{base} · {h}"),
        _ => base,
    }
}

pub fn a_markdown(l: &Lectura) -> String {
    let fin = l.desde + l.bloques.len();
    let mut s = format!(
        "# {}\n\n`{}` · bloques {}–{} de {}\n\n",
        l.titulo,
        l.id_publico,
        if l.bloques.is_empty() { l.desde } else { l.desde + 1 },
        fin,
        l.total
    );
    for b in &l.bloques {
        match b {
            Bloque::Usuario { hora, texto } => {
                s.push_str(&format!("{}\n\n{texto}\n\n", con_hora("### 👤 Tú".into(), hora)));
            }
            Bloque::Asistente { modelo, hora, texto } => {
                let nombre = modelo.as_deref().filter(|m| !m.is_empty()).unwrap_or("Asistente");
                s.push_str(&format!("{}\n\n{texto}\n\n", con_hora(format!("### 🤖 {nombre}"), hora)));
            }
            Bloque::Herramientas { n, archivos } => {
                s.push_str(&format!("> 🔧 {n} herramientas · {archivos} archivos\n\n"));
            }
            Bloque::Razonamiento { texto, desplegado } => {
                if *desplegado {
                    s.push_str(&format!("### 💭 Razonamiento\n\n{texto}\n\n"));
                } else {
                    s.push_str(&format!("> 💭 Razonamiento ({} caracteres)\n\n", texto.chars().count()));
                }
            }
            Bloque::Herramienta { texto } => {
                s.push_str(&format!("### 🔧 Herramienta\n\n{texto}\n\n"));
            }
            Bloque::Subagente { id_publico, descripcion } => {
                s.push_str(&format!("> 🤖 Subagente {id_publico}: {descripcion}\n\n"));
            }
            Bloque::Sistema { texto } => {
                s.push_str(&format!("> ⚙️ Sistema: {}\n\n", texto.replace('\n', "\n> ")));
            }
        }
    }
    if fin < l.total {
        s.push_str(&format!("… siguiente tramo: nisaba ver {} --desde {fin}\n", l.id_publico));
    }
    s
}
