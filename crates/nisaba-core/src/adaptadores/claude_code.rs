use super::{avisar, recortar};
use crate::modelo::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

const TIPOS_IGNORADOS: &[&str] = &[
    "attachment",
    "api-request",
    "api-request-blob",
    "api-request-shape",
    "custom-title",
    "last-prompt",
    "atis-latch",
    "agent-name",
    "queue-operation",
    "mode",
    "file-history-snapshot",
    "file-history-delta",
    "cost-state",
    "summary",
];

const PREFIJOS_OCULTOS: &[&str] = &[
    "<command-",
    "<local-command",
    "<system-reminder>",
    "<task-notification>",
    "Base directory for this skill:",
    "Caveat:",
];

/// Todos los `*.jsonl` bajo `raiz` (incluidos `*/subagents/agent-*.jsonl`), ordenados.
pub fn descubrir(raiz: &Path) -> Vec<PathBuf> {
    let mut salida = Vec::new();
    recorrer(raiz, &mut salida);
    salida.sort();
    salida
}

fn recorrer(dir: &Path, salida: &mut Vec<PathBuf>) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entradas.flatten() {
        let ruta = e.path();
        let Ok(tipo) = e.file_type() else { continue };
        if tipo.is_dir() {
            recorrer(&ruta, salida);
        } else if tipo.is_file() && ruta.extension().is_some_and(|x| x == "jsonl") {
            salida.push(ruta);
        }
    }
}

/// Texto de un `tool_result.content`: string o lista de bloques `text`.
fn texto_resultado(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(bloques)) => bloques
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn partes_de(content: Option<&Value>, avisos: &mut Vec<String>) -> Vec<Parte> {
    let mut partes = Vec::new();
    match content {
        Some(Value::String(s)) => partes.push(Parte {
            tipo: TipoParte::Texto,
            contenido: s.clone(),
        }),
        Some(Value::Array(bloques)) => {
            for b in bloques {
                let tipo = b.get("type").and_then(Value::as_str).unwrap_or("");
                match tipo {
                    "text" => {
                        let t = b.get("text").and_then(Value::as_str).unwrap_or("");
                        partes.push(Parte {
                            tipo: TipoParte::Texto,
                            contenido: t.to_string(),
                        });
                    }
                    "thinking" => {
                        let t = b.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if !t.is_empty() {
                            partes.push(Parte {
                                tipo: TipoParte::Razonamiento,
                                contenido: t.to_string(),
                            });
                        }
                    }
                    "tool_use" => {
                        let nombre = b.get("name").and_then(Value::as_str).unwrap_or("?");
                        let input = b.get("input").map(Value::to_string).unwrap_or_default();
                        partes.push(Parte {
                            tipo: TipoParte::Herramienta,
                            contenido: recortar(format!("{nombre}: {input}")),
                        });
                    }
                    "tool_result" => partes.push(Parte {
                        tipo: TipoParte::Herramienta,
                        contenido: recortar(texto_resultado(b.get("content"))),
                    }),
                    "image" => {
                        let mt = b
                            .pointer("/source/media_type")
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        partes.push(Parte {
                            tipo: TipoParte::Imagen,
                            contenido: format!("[imagen {mt}]"),
                        });
                    }
                    otro => avisar(avisos, format!("bloque desconocido: {otro}")),
                }
            }
        }
        _ => {}
    }
    partes
}

fn texto_inicial(partes: &[Parte]) -> Option<&str> {
    partes
        .iter()
        .find(|p| p.tipo == TipoParte::Texto)
        .map(|p| p.contenido.as_str())
}

fn texto(v: &Value, clave: &str) -> Option<String> {
    v.get(clave).and_then(Value::as_str).map(str::to_string)
}

pub fn parsear(ruta: &Path) -> Result<ConversacionParseada, String> {
    let bytes = std::fs::read(ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
    let contenido = String::from_utf8_lossy(&bytes);
    let lineas: Vec<(usize, &str)> = contenido
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .collect();

    let stem = ruta
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let carpeta = ruta.parent();
    let es_subagente = carpeta
        .and_then(|d| d.file_name())
        .is_some_and(|n| n == "subagents");
    let padre_original = if es_subagente {
        carpeta
            .and_then(|d| d.parent())
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str())
            .map(str::to_string)
    } else {
        None
    };
    let descripcion = if es_subagente {
        std::fs::read_to_string(ruta.with_extension("meta.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| texto(&v, "description"))
    } else {
        None
    };

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut hoja: Option<String> = None;
    let mut mensajes = Vec::new();
    let mut avisos = Vec::new();

    for (pos, (num, linea)) in lineas.iter().enumerate() {
        let v: Value = match serde_json::from_str(linea) {
            Ok(v) => v,
            // La última línea puede estar escribiéndose todavía.
            Err(_) if pos == lineas.len() - 1 => continue,
            Err(e) => return Err(format!("línea {}: {e}", num + 1)),
        };
        if session_id.is_none() {
            session_id = texto(&v, "sessionId").filter(|s| !s.is_empty());
        }
        if cwd.is_none() {
            cwd = texto(&v, "cwd").filter(|s| !s.is_empty());
        }
        let tipo = v.get("type").and_then(Value::as_str).unwrap_or("");
        match tipo {
            "last-prompt" => {
                if let Some(h) = texto(&v, "leafUuid") {
                    hoja = Some(h);
                }
            }
            "user" | "assistant" | "system" => {
                let rol = match tipo {
                    "user" => Rol::Usuario,
                    "assistant" => Rol::Asistente,
                    _ => Rol::Sistema,
                };
                let content = v.pointer("/message/content").or_else(|| v.get("content"));
                let mut partes = partes_de(content, &mut avisos);
                if rol == Rol::Sistema {
                    for p in &mut partes {
                        if p.tipo == TipoParte::Texto {
                            p.tipo = TipoParte::Sistema;
                        }
                    }
                }
                let marcado = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
                let inyectado = rol == Rol::Usuario
                    && texto_inicial(&partes).is_some_and(|t| {
                        let t = t.trim_start();
                        PREFIJOS_OCULTOS.iter().any(|p| t.starts_with(p))
                    });
                mensajes.push(Mensaje {
                    uuid: texto(&v, "uuid"),
                    id_padre: texto(&v, "parentUuid"),
                    rol,
                    modelo: v
                        .pointer("/message/model")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    hora: texto(&v, "timestamp"),
                    oculto: marcado("isMeta")
                        || marcado("isCompactSummary")
                        || rol == Rol::Sistema
                        || inyectado,
                    partes,
                });
            }
            t if TIPOS_IGNORADOS.contains(&t) => {}
            otro => avisar(&mut avisos, format!("tipo desconocido: {otro}")),
        }
    }

    let id_original = if es_subagente {
        stem
    } else {
        session_id.unwrap_or(stem)
    };

    Ok(ConversacionParseada {
        fuente: Fuente::ClaudeCode,
        id_original,
        cwd,
        padre_original,
        descripcion,
        hoja,
        mensajes,
        avisos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = "-home-u-Proyectos-Demo";
    const SESION: &str = "a1b2c3d4-0000-4000-8000-000000000001";

    fn raiz() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude")
    }
    fn principal() -> ConversacionParseada {
        parsear(&raiz().join(DEMO).join(format!("{SESION}.jsonl"))).unwrap()
    }

    #[test]
    fn parsea_mensajes_visibles() {
        let c = principal();
        let visibles: Vec<_> = c.mensajes.iter().filter(|m| !m.oculto).collect();
        assert_eq!(visibles.len(), 4);
        assert_eq!(visibles[0].rol, Rol::Usuario);
        assert!(visibles[0].partes[0].contenido.contains("dosage schedule"));
        assert_eq!(c.cwd.as_deref(), Some("/home/u/Proyectos/Demo"));
        assert_eq!(c.id_original, SESION);
        assert_eq!(c.hoja.as_deref(), Some("a2"));
    }

    #[test]
    fn oculta_inyectados() {
        let c = principal();
        let m = c
            .mensajes
            .iter()
            .find(|m| {
                m.partes
                    .iter()
                    .any(|p| p.contenido.contains("ornitorrinco"))
            })
            .unwrap();
        assert!(m.oculto);
    }

    #[test]
    fn omite_thinking_vacio() {
        let c = principal();
        assert!(!c
            .mensajes
            .iter()
            .flat_map(|m| &m.partes)
            .any(|p| p.tipo == TipoParte::Razonamiento && p.contenido.is_empty()));
    }

    #[test]
    fn herramienta_resumida() {
        let c = principal();
        assert!(c
            .mensajes
            .iter()
            .flat_map(|m| &m.partes)
            .any(|p| p.tipo == TipoParte::Herramienta && p.contenido.starts_with("Read: ")));
        assert!(c
            .mensajes
            .iter()
            .flat_map(|m| &m.partes)
            .any(|p| p.tipo == TipoParte::Herramienta && p.contenido.contains("zanahoria42")));
    }

    #[test]
    fn ultima_linea_cortada_no_es_error() {
        assert!(parsear(&raiz().join(DEMO).join(format!("{SESION}.jsonl"))).is_ok());
    }

    #[test]
    fn json_roto_en_medio_es_error() {
        let e = parsear(&raiz().join(DEMO).join("roto.jsonl")).unwrap_err();
        assert!(e.contains("línea 2"), "{e}");
    }

    #[test]
    fn avisa_tipo_desconocido() {
        assert_eq!(principal().avisos, vec!["tipo desconocido: desconocido-x"]);
    }

    #[test]
    fn subagente() {
        let r = raiz()
            .join(DEMO)
            .join(SESION)
            .join("subagents/agent-zz.jsonl");
        let c = parsear(&r).unwrap();
        assert_eq!(c.id_original, "agent-zz");
        assert_eq!(c.padre_original.as_deref(), Some(SESION));
        assert_eq!(c.descripcion.as_deref(), Some("Revisión final"));
        assert_eq!(c.mensajes.len(), 2);
    }

    #[test]
    fn descubrir_incluye_subagentes() {
        assert_eq!(descubrir(&raiz()).len(), 3);
    }

    #[test]
    fn recorta_herramientas_enormes() {
        let d = tempfile::tempdir().unwrap();
        let grande = "x".repeat(100_000);
        let linea = format!(
            r#"{{"type":"user","uuid":"u","sessionId":"s","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t","content":"{grande}"}},{{"type":"image","source":{{"type":"base64","media_type":"image/webp","data":"AAAA"}}}}]}}}}"#
        );
        let ruta = d.path().join("s.jsonl");
        std::fs::write(&ruta, linea + "\n").unwrap();
        let c = parsear(&ruta).unwrap();
        let p = &c.mensajes[0].partes;
        assert!(p[0].contenido.ends_with("\n…[recortado]"));
        assert!(p[0].contenido.len() <= 64 * 1024 + 20);
        assert_eq!(p[1].contenido, "[imagen image/webp]");
    }
}
