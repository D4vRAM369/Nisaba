//! Búsqueda FTS5 agrupada por sesión (los subagentes cuentan en su sesión madre).

use crate::modelo::Fuente;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;

/// Filtros de búsqueda. `razonamiento` y `codigo` amplían los tipos de parte
/// que se consultan (por defecto solo `texto`).
#[derive(Debug, Clone)]
pub struct Filtros {
    pub fuente: Option<Fuente>,
    pub proyecto: Option<String>,
    /// Fecha ISO; se compara `fin >= desde` como texto.
    pub desde: Option<String>,
    pub razonamiento: bool,
    pub codigo: bool,
    pub limite: usize,
}

impl Default for Filtros {
    fn default() -> Self {
        Filtros {
            fuente: None,
            proyecto: None,
            desde: None,
            razonamiento: false,
            codigo: false,
            limite: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Resultado {
    pub id_publico: String,
    pub titulo: String,
    pub fuente: Fuente,
    pub proyecto: Option<String>,
    pub fin: Option<String>,
    pub coincidencias: i64,
    pub fragmento: String,
    pub orden_mensaje: i64,
    /// Carpeta de trabajo de la sesión (madre, si es un subagente).
    pub cwd: Option<String>,
    pub id_original: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Respuesta {
    pub resultados: Vec<Resultado>,
    pub total_sesiones: i64,
    pub sugerencia: Option<String>,
}

/// Traduce lo que escribió la persona a una consulta FTS5 segura: cada término
/// va entre comillas (neutraliza AND/OR/NOT/-/:), `término*` es prefijo.
pub fn consulta_fts(entrada: &str) -> Option<String> {
    let mut terminos: Vec<String> = Vec::new();
    let mut resto = entrada;
    while !resto.is_empty() {
        resto = resto.trim_start();
        if resto.is_empty() {
            break;
        }
        if let Some(despues) = resto.strip_prefix('"') {
            // Frase; si no se cierra, el resto entero es la frase.
            let (frase, siguiente) = match despues.find('"') {
                Some(i) => (&despues[..i], &despues[i + 1..]),
                None => (despues, ""),
            };
            let frase = frase.trim();
            if !frase.is_empty() {
                terminos.push(format!("\"{frase}\""));
            }
            resto = siguiente;
        } else {
            let fin = resto.find(char::is_whitespace).unwrap_or(resto.len());
            let (palabra, siguiente) = resto.split_at(fin);
            resto = siguiente;
            let limpio = palabra.replace('"', "");
            let (nucleo, prefijo) = match limpio.strip_suffix('*') {
                Some(n) => (n.to_string(), true),
                None => (limpio, false),
            };
            let nucleo = nucleo.trim_end_matches('*');
            if nucleo.is_empty() {
                continue;
            }
            terminos.push(format!("\"{nucleo}\"{}", if prefijo { "*" } else { "" }));
        }
    }
    if terminos.is_empty() {
        None
    } else {
        Some(terminos.join(" "))
    }
}

const DICCIONARIO: &[(&str, &str)] = &[
    ("medicación", "medication"),
    ("dosis", "dose"),
    ("horario", "schedule"),
    ("error", "bug"),
    ("prueba", "test"),
    ("base de datos", "database"),
    ("ventana", "window"),
    ("botón", "button"),
    ("permiso", "permission"),
    ("gancho", "hook"),
    ("subtítulo", "subtitle"),
    ("tarjeta", "card"),
    ("búsqueda", "search"),
    ("indexar", "index"),
    ("compilar", "build"),
];

fn sin_acentos(c: char) -> char {
    match c {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ú' | 'ü' => 'u',
        _ => c,
    }
}

/// Minúsculas, sin acentos, solo palabras separadas por un espacio y rodeadas de espacios.
fn normalizar(s: &str) -> String {
    let limpio: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                sin_acentos(c)
            } else {
                ' '
            }
        })
        .collect();
    format!(
        " {} ",
        limpio.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

fn sugerir_traduccion(entrada: &str) -> Option<String> {
    let texto = normalizar(entrada);
    DICCIONARIO
        .iter()
        .find_map(|(es, en)| {
            if texto.contains(&normalizar(es)) {
                Some(*en)
            } else if texto.contains(&normalizar(en)) {
                Some(*es)
            } else {
                None
            }
        })
        .map(|t| format!("Sin resultados. ¿Probar «{t}»?"))
}

struct Grupo {
    parte_id: i64,
    mejor: f64,
    orden: i64,
    coincidencias: i64,
    fuente: String,
    id_original: String,
    titulo: Option<String>,
    proyecto: Option<String>,
    fin: Option<String>,
    dias: f64,
    cwd: Option<String>,
}

pub fn buscar(
    conn: &Connection,
    entrada: &str,
    f: &Filtros,
    canal: &str,
) -> rusqlite::Result<Respuesta> {
    contar_uso(conn, canal)?;
    buscar_sin_contar(conn, entrada, f)
}

/// Suma una búsqueda al contador local del día para ese canal (spec §8).
pub fn contar_uso(conn: &Connection, canal: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO uso(dia, canal, busquedas) VALUES (date('now','localtime'), ?1, 1)
         ON CONFLICT(dia, canal) DO UPDATE SET busquedas = busquedas + 1",
        [canal],
    )?;
    Ok(())
}

/// Como `buscar`, pero sin tocar el contador: la GUI busca mientras escribes y solo cuenta al abrir un resultado.
pub fn buscar_sin_contar(
    conn: &Connection,
    entrada: &str,
    f: &Filtros,
) -> rusqlite::Result<Respuesta> {
    let Some(fts) = consulta_fts(entrada) else {
        return Ok(Respuesta {
            resultados: vec![],
            total_sesiones: 0,
            sugerencia: None,
        });
    };

    let mut tipos = vec!["'texto'"];
    if f.razonamiento {
        tipos.push("'razonamiento'");
    }
    if f.codigo {
        tipos.push("'herramienta'");
    }
    // Los filtros y los datos mostrados son los de la sesión madre.
    let sql = format!(
        "SELECT g.id, p.id, m.orden, bm25(partes_fts), g.fuente, g.id_original, g.titulo, g.proyecto, g.fin,
                COALESCE(julianday('now') - julianday(g.fin), 3650.0), g.cwd
         FROM partes_fts
         JOIN partes p ON p.id = partes_fts.rowid
         JOIN mensajes m ON m.id = p.mensaje_id
         JOIN conversaciones c ON c.id = m.conversacion_id
         JOIN conversaciones g ON g.id = COALESCE(c.padre_id, c.id)
         WHERE partes_fts MATCH ?1 AND m.oculto = 0 AND p.tipo IN ({})
           AND (?2 IS NULL OR g.fuente = ?2)
           AND (?3 IS NULL OR g.proyecto = ?3)
           AND (?4 IS NULL OR g.fin >= ?4)",
        tipos.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let filas = stmt.query_map(
        params![fts, f.fuente.map(|x| x.as_str()), f.proyecto, f.desde],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Grupo {
                    parte_id: r.get(1)?,
                    orden: r.get(2)?,
                    mejor: r.get(3)?,
                    coincidencias: 1,
                    fuente: r.get(4)?,
                    id_original: r.get(5)?,
                    titulo: r.get(6)?,
                    proyecto: r.get(7)?,
                    fin: r.get(8)?,
                    dias: r.get(9)?,
                    cwd: r.get(10)?,
                },
            ))
        },
    )?;
    let mut grupos: HashMap<i64, Grupo> = HashMap::new();
    for fila in filas {
        let (gid, nuevo) = fila?;
        match grupos.get_mut(&gid) {
            Some(g) => {
                g.coincidencias += 1;
                if nuevo.mejor < g.mejor {
                    g.mejor = nuevo.mejor;
                    g.parte_id = nuevo.parte_id;
                    g.orden = nuevo.orden;
                }
            }
            None => {
                grupos.insert(gid, nuevo);
            }
        }
    }
    let total_sesiones = grupos.len() as i64;

    // bm25 es negativo (más negativo = mejor); lo reciente pesa hasta un 50 % más.
    let puntaje = |g: &Grupo| g.mejor * (1.0 + 0.5 * (-g.dias / 60.0).exp());
    let mut ordenados: Vec<(i64, Grupo)> = grupos.into_iter().collect();
    ordenados.sort_by(|a, b| puntaje(&a.1).total_cmp(&puntaje(&b.1)).then(a.0.cmp(&b.0)));
    ordenados.truncate(f.limite);

    let mut resultados = Vec::with_capacity(ordenados.len());
    for (_, g) in ordenados {
        let fragmento: String = conn.query_row(
            "SELECT snippet(partes_fts, 0, '«', '»', '…', 12) FROM partes_fts
             WHERE partes_fts MATCH ?1 AND rowid = ?2",
            params![fts, g.parte_id],
            |r| r.get(0),
        )?;
        let Some(fuente) = Fuente::desde_str(&g.fuente) else {
            continue;
        };
        resultados.push(Resultado {
            id_publico: id_publico(fuente, &g.id_original),
            titulo: g.titulo.unwrap_or_default(),
            fuente,
            proyecto: g.proyecto,
            fin: g.fin,
            coincidencias: g.coincidencias,
            fragmento,
            orden_mensaje: g.orden,
            cwd: g.cwd,
            id_original: g.id_original,
        });
    }

    let sugerencia = if total_sesiones == 0 {
        sugerir_traduccion(entrada)
    } else if total_sesiones > 50 && f.proyecto.is_none() {
        Some("Muchas sesiones. Añade --proyecto.".to_string())
    } else {
        None
    };
    Ok(Respuesta {
        resultados,
        total_sesiones,
        sugerencia,
    })
}

/// Los ids de Codex (UUIDv7) y de subagentes (`agent-…`) comparten el comienzo:
/// para distinguirlos se usan sus últimos caracteres.
fn por_el_final(fuente: Fuente, id_original: &str) -> bool {
    fuente == Fuente::Codex || id_original.starts_with("agent-")
}

/// Id corto que ve la persona: `{prefijo}-{8 caracteres del id original}`
/// (los primeros, o los últimos si el comienzo no distingue).
pub fn id_publico(fuente: Fuente, id_original: &str) -> String {
    let corto: String = if por_el_final(fuente, id_original) {
        let n = id_original.chars().count();
        id_original.chars().skip(n.saturating_sub(8)).collect()
    } else {
        id_original.chars().take(8).collect()
    };
    format!("{}-{corto}", fuente.prefijo())
}

/// Resultado de resolver un id público.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolucion {
    Uno(i64),
    Ninguno,
    /// Más de una conversación encaja; trae sus ids públicos completos.
    Ambiguo(Vec<String>),
}

/// `cc-a1b2c3d4` → conversación. Cada fila se compara por el extremo que usa
/// `id_publico` para ella (`por_el_final`); si encajan varias no se elige ninguna.
pub fn resolver(conn: &Connection, id_publico_txt: &str) -> rusqlite::Result<Resolucion> {
    let Some((prefijo, resto)) = id_publico_txt.split_once('-') else {
        return Ok(Resolucion::Ninguno);
    };
    let Some(fuente) = [Fuente::ClaudeCode, Fuente::Codex]
        .into_iter()
        .find(|f| f.prefijo() == prefijo)
    else {
        return Ok(Resolucion::Ninguno);
    };
    if resto.is_empty() {
        return Ok(Resolucion::Ninguno);
    }
    // substr en vez de LIKE: los ids pueden llevar `_` o `%`.
    let mut stmt = conn.prepare(
        "SELECT id, id_original FROM conversaciones
         WHERE fuente = ?1
           AND CASE WHEN fuente = 'codex' OR substr(id_original, 1, 6) = 'agent-'
                    THEN substr(id_original, -?3) ELSE substr(id_original, 1, ?3) END = ?2
         ORDER BY id LIMIT 5",
    )?;
    let filas: Vec<(i64, String)> = stmt
        .query_map(
            params![fuente.as_str(), resto, resto.chars().count() as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<Result<_, _>>()?;
    Ok(match filas.len() {
        0 => Resolucion::Ninguno,
        1 => Resolucion::Uno(filas[0].0),
        _ => Resolucion::Ambiguo(
            filas
                .iter()
                .map(|(_, o)| format!("{prefijo}-{o}"))
                .collect(),
        ),
    })
}

/// Atajo: `Some` solo si el id es único.
pub fn resolver_id(conn: &Connection, id_publico: &str) -> rusqlite::Result<Option<i64>> {
    Ok(match resolver(conn, id_publico)? {
        Resolucion::Uno(i) => Some(i),
        _ => None,
    })
}

/// Lista las sesiones madre sin contar una consulta de búsqueda.
pub fn recientes(conn: &Connection, f: &Filtros) -> rusqlite::Result<Respuesta> {
    recientes_desde(conn, f, 0)
}

pub fn recientes_desde(
    conn: &Connection,
    f: &Filtros,
    desplazamiento: usize,
) -> rusqlite::Result<Respuesta> {
    const CONDICION: &str = "c.padre_id IS NULL AND c.fuente IN ('claude-code','codex')
        AND (?1 IS NULL OR c.fuente=?1)
        AND (?2 IS NULL OR c.proyecto=?2)
        AND (?3 IS NULL OR c.fin>=?3)";
    let fuente = f.fuente.map(|f| f.as_str());
    let total_sesiones = conn.query_row(
        &format!("SELECT COUNT(*) FROM conversaciones c WHERE {CONDICION}"),
        params![fuente, f.proyecto, f.desde],
        |r| r.get(0),
    )?;
    let mut st = conn.prepare(&format!(
        "SELECT c.fuente,c.id_original,COALESCE(c.titulo,''),c.proyecto,c.fin,
         COALESCE((SELECT substr(p.contenido,1,180) FROM mensajes m JOIN partes p ON p.mensaje_id=m.id
           WHERE m.conversacion_id=c.id AND m.oculto=0 AND p.tipo='texto'
           ORDER BY m.orden,p.orden LIMIT 1),''), c.cwd
         FROM conversaciones c WHERE {CONDICION} ORDER BY c.fin DESC,c.id DESC LIMIT ?4 OFFSET ?5"
    ))?;
    let resultados = st
        .query_map(
            params![
                fuente,
                f.proyecto,
                f.desde,
                i64::try_from(f.limite).unwrap_or(i64::MAX),
                i64::try_from(desplazamiento).unwrap_or(i64::MAX)
            ],
            |r| {
                let nombre: String = r.get(0)?;
                let fuente = Fuente::desde_str(&nombre).ok_or(rusqlite::Error::InvalidQuery)?;
                let id: String = r.get(1)?;
                Ok(Resultado {
                    id_publico: id_publico(fuente, &id),
                    fuente,
                    titulo: r.get(2)?,
                    proyecto: r.get(3)?,
                    fin: r.get(4)?,
                    fragmento: r.get(5)?,
                    coincidencias: 0,
                    orden_mensaje: 0,
                    cwd: r.get(6)?,
                    id_original: id,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Respuesta {
        resultados,
        total_sesiones,
        sugerencia: None,
    })
}
