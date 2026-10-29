//! Servidor MCP a mano: JSON-RPC 2.0, un mensaje por línea, solo stdio.
//! stdout es exclusivamente JSON-RPC; todo log va a stderr.

use crate::formato::texto_busqueda;
use nisaba_core::busqueda::{buscar, resolver_id, Filtros};
use nisaba_core::db;
use nisaba_core::indexador::{indexar, Progreso, ResultadoIndexado, Rutas};
use nisaba_core::lector::{a_markdown, leer, Tramo};
use nisaba_core::modelo::Fuente;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const VERSION_POR_DEFECTO: &str = "2025-06-18";

pub fn servir(rutas: &Rutas) -> std::io::Result<()> {
    let mut conn = db::abrir(&rutas.home.join("tablilla.db"))
        .map_err(|e| std::io::Error::other(format!("no puedo abrir la tablilla: {e}")))?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for linea in stdin.lock().lines() {
        let linea = linea?;
        if linea.trim().is_empty() {
            continue;
        }
        if let Some(resp) = procesar(&linea, &mut conn, rutas) {
            writeln!(stdout, "{resp}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn error(id: Value, codigo: i64, mensaje: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":codigo,"message":mensaje}})
}

fn exito(id: Value, resultado: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":resultado})
}

/// Devuelve la respuesta a una línea, o `None` si era una notificación.
fn procesar(linea: &str, conn: &mut Connection, rutas: &Rutas) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(linea) else {
        return Some(error(Value::Null, -32700, "JSON inválido"));
    };
    let Some(obj) = msg.as_object() else {
        return Some(error(Value::Null, -32600, "petición inválida"));
    };
    let id = obj.get("id").cloned();
    let Some(metodo) = obj.get("method").and_then(Value::as_str) else {
        return Some(error(id.unwrap_or(Value::Null), -32600, "petición inválida: falta method"));
    };
    // Sin id es una notificación: nunca se responde.
    let id = id?;
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Some(match metodo {
        "initialize" => {
            let v = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(VERSION_POR_DEFECTO);
            exito(
                id,
                json!({
                    "protocolVersion": v,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "nisaba", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
        }
        "ping" => exito(id, json!({})),
        "tools/list" => exito(id, json!({"tools": herramientas()})),
        "tools/call" => exito(id, llamar(&params, conn, rutas)),
        _ => error(id, -32601, &format!("método desconocido: {metodo}")),
    })
}

fn herramientas() -> Value {
    json!([
        {
            "name": "buscar_conversaciones",
            "description": "Busca en las conversaciones pasadas de Brian con IAs (Claude Code, Codex) por palabra o frase. Úsala cuando pregunte '¿recuerdas cuando…?'. Devuelve sesiones con id, título y fragmento.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "consulta": {"type": "string", "description": "Palabra o frase a buscar"},
                    "fuente": {"type": "string", "description": "claude-code | codex"},
                    "proyecto": {"type": "string"},
                    "razonamiento": {"type": "boolean", "description": "Incluir bloques de razonamiento"},
                    "codigo": {"type": "boolean", "description": "Incluir llamadas a herramientas y código"},
                    "limite": {"type": "integer", "description": "Máximo de sesiones (por defecto 10)"}
                },
                "required": ["consulta"]
            }
        },
        {
            "name": "leer_conversacion",
            "description": "Lee una conversación por tramos en modo solo conversación. Por defecto ~20 bloques centrados en la coincidencia.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Id de la sesión, como sale en la búsqueda"},
                    "desde": {"type": "integer", "description": "Bloque desde el que empezar"},
                    "cantidad": {"type": "integer", "description": "Cuántos bloques (por defecto 20)"},
                    "alrededor_de": {"type": "integer", "description": "Orden de mensaje a centrar (el alrededor_de de la búsqueda)"},
                    "todo": {"type": "boolean", "description": "Mostrar todo: ocultos, sistema y cada parte"}
                },
                "required": ["id"]
            }
        }
    ])
}

fn texto(t: String, es_error: bool) -> Value {
    if es_error {
        json!({"content":[{"type":"text","text":t}],"isError":true})
    } else {
        json!({"content":[{"type":"text","text":t}]})
    }
}

fn llamar(params: &Value, conn: &mut Connection, rutas: &Rutas) -> Value {
    let nombre = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if nombre != "buscar_conversaciones" && nombre != "leer_conversacion" {
        return texto(format!("Herramienta desconocida: {nombre:?}. Usa buscar_conversaciones o leer_conversacion."), true);
    }
    // Validar antes de indexar: un argumento ausente no debe costar una indexación.
    let r = if nombre == "buscar_conversaciones" {
        let Some(consulta) = args.get("consulta").and_then(Value::as_str) else {
            return texto("Falta el argumento obligatorio «consulta» (string).".into(), true);
        };
        indexar_antes(conn, rutas);
        buscar_tool(conn, consulta, &args)
    } else {
        let Some(id) = args.get("id").and_then(Value::as_str) else {
            return texto("Falta el argumento obligatorio «id» (string).".into(), true);
        };
        indexar_antes(conn, rutas);
        leer_tool(conn, id, &args)
    };
    match r {
        Ok(t) => texto(t, false),
        Err(e) => texto(e, true),
    }
}

/// Indexa lo nuevo. `Ocupado` sigue con lo que hay; `Fallo` avisa por stderr y sigue.
fn indexar_antes(conn: &mut Connection, rutas: &Rutas) {
    if let ResultadoIndexado::Fallo(e) = indexar(conn, rutas, &mut |_p: Progreso| {}) {
        eprintln!("Error al indexar: {e}");
    }
}

fn entero(args: &Value, k: &str) -> Option<i64> {
    args.get(k).and_then(Value::as_i64)
}

fn buscar_tool(conn: &Connection, consulta: &str, args: &Value) -> Result<String, String> {
    let fuente = match args.get("fuente").and_then(Value::as_str) {
        None => None,
        Some(s) => Some(Fuente::desde_str(s).ok_or("Fuente desconocida: usa claude-code o codex.")?),
    };
    let mut f = Filtros {
        fuente,
        proyecto: args.get("proyecto").and_then(Value::as_str).map(String::from),
        razonamiento: args.get("razonamiento").and_then(Value::as_bool).unwrap_or(false),
        codigo: args.get("codigo").and_then(Value::as_bool).unwrap_or(false),
        ..Default::default()
    };
    if let Some(l) = entero(args, "limite") {
        f.limite = l.max(1) as usize;
    }
    let r = buscar(conn, consulta, &f, "mcp").map_err(|e| e.to_string())?;
    if r.resultados.is_empty() && r.sugerencia.is_none() {
        return Ok("Sin resultados.".into());
    }
    Ok(texto_busqueda(&r, false, true))
}

fn leer_tool(conn: &Connection, id: &str, args: &Value) -> Result<String, String> {
    let Some(cid) = resolver_id(conn, id).map_err(|e| e.to_string())? else {
        return Err(format!("No encuentro la conversación {id}"));
    };
    let mut t = Tramo::default();
    t.desde = entero(args, "desde").map(|n| n.max(0) as usize);
    if let Some(c) = entero(args, "cantidad") {
        t.cantidad = c.max(1) as usize;
    }
    t.alrededor_de = entero(args, "alrededor_de");
    t.todo = args.get("todo").and_then(Value::as_bool).unwrap_or(false);
    let l = leer(conn, cid, &t).map_err(|e| e.to_string())?;
    Ok(a_markdown(&l))
}
