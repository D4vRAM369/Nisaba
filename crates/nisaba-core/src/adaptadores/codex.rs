use super::{avisar, recortar};
use crate::modelo::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

const TIPOS_IGNORADOS: &[&str] = &[
    "session_meta",
    "event_msg",
    "turn_context",
    "world_state",
    "token_usage_record",
    // Marca de turno sin texto y copia del historial al compactar (ya está en el archivo).
    "inter_agent_communication_metadata",
    "compacted",
];

const PREFIJOS_OCULTOS: &[&str] = &[
    "# AGENTS.md instructions",
    "<environment_context>",
    "<INSTRUCTIONS>",
    "The following is the Code",
    "<external_",
    "<task-notification>",
    "<command-message>",
    "<send_user_message_",
    "[external unsupported",
    // Vistos en datos reales (cientos de apariciones).
    "<turn_aborted>",
    "<recommended_plugins>",
    "<subagent_notification>",
    "<system-reminder>",
    "<user_instructions>",
    "<command-name>",
    "This session is being continued",
    "[Request interrupted",
];

/// `rollout-*.jsonl` bajo `raiz/sessions` y `raiz/archived_sessions`, ordenados.
pub fn descubrir(raiz: &Path) -> Vec<PathBuf> {
    let mut salida = Vec::new();
    for sub in ["sessions", "archived_sessions"] {
        recorrer(&raiz.join(sub), &mut salida);
    }
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
        } else if tipo.is_file() {
            let nombre = ruta.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if nombre.starts_with("rollout-")
                && (nombre.ends_with(".jsonl") || nombre.ends_with(".jsonl.zst"))
            {
                salida.push(ruta);
            }
        }
    }
}

fn texto(v: &Value, clave: &str) -> Option<String> {
    v.get(clave).and_then(Value::as_str).map(str::to_string)
}

/// Une los `text` de una lista de bloques; si es un string, lo devuelve tal cual.
fn texto_bloques(v: Option<&Value>) -> String {
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

fn parte(tipo: TipoParte, contenido: String) -> Vec<Parte> {
    vec![Parte { tipo, contenido }]
}

/// Bytes del rollout; Codex comprime los antiguos como `*.jsonl.zst`.
fn leer_bytes(ruta: &Path) -> Result<Vec<u8>, String> {
    let crudo = std::fs::read(ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
    if !ruta.extension().is_some_and(|x| x == "zst") {
        return Ok(crudo);
    }
    use std::io::Read;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(crudo.as_slice())
        .map_err(|e| format!("{}: zstd: {e}", ruta.display()))?;
    let mut salida = Vec::new();
    dec.read_to_end(&mut salida)
        .map_err(|e| format!("{}: zstd: {e}", ruta.display()))?;
    Ok(salida)
}

/// `cwd` de `session_meta`, leyendo (y descomprimiendo) solo hasta encontrarlo.
pub fn cwd_cabecera(ruta: &Path) -> Option<String> {
    let f = std::fs::File::open(ruta).ok()?;
    let de_meta = |v: &Value| {
        (v.get("type").and_then(Value::as_str) == Some("session_meta"))
            .then(|| v.get("payload").and_then(|p| texto(p, "cwd")))
            .flatten()
    };
    if ruta.extension().is_some_and(|x| x == "zst") {
        let dec = ruzstd::decoding::StreamingDecoder::new(f).ok()?;
        crate::adaptadores::primer_cwd(std::io::BufReader::new(dec), de_meta)
    } else {
        crate::adaptadores::primer_cwd(std::io::BufReader::new(f), de_meta)
    }
}

pub fn parsear(ruta: &Path) -> Result<ConversacionParseada, String> {
    let bytes = leer_bytes(ruta)?;
    let contenido = String::from_utf8_lossy(&bytes);
    let lineas: Vec<(usize, &str)> = contenido
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .collect();

    let mut id_original: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut modelo: Option<String> = None;
    let mut mensajes = Vec::new();
    let mut avisos = Vec::new();

    for (pos, (num, linea)) in lineas.iter().enumerate() {
        let v: Value = match serde_json::from_str(linea) {
            Ok(v) => v,
            // La última línea puede estar escribiéndose todavía.
            Err(_) if pos == lineas.len() - 1 => continue,
            Err(e) => return Err(format!("línea {}: {e}", num + 1)),
        };
        let tipo = v.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = v.get("payload").unwrap_or(&Value::Null);
        let hora = texto(&v, "timestamp");

        match tipo {
            "session_meta" => {
                if id_original.is_none() {
                    id_original = texto(payload, "id").filter(|s| !s.is_empty());
                    cwd = texto(payload, "cwd").filter(|s| !s.is_empty());
                }
            }
            "event_msg" => {
                if payload.get("type").and_then(Value::as_str) == Some("thread_settings_applied") {
                    if let Some(m) = payload
                        .pointer("/thread_settings/model")
                        .and_then(Value::as_str)
                    {
                        modelo = Some(m.to_string());
                    }
                }
            }
            "response_item" => {
                let ptipo = payload.get("type").and_then(Value::as_str).unwrap_or("");
                let (rol, partes, oculto) = match ptipo {
                    "message" => {
                        let rol = match payload.get("role").and_then(Value::as_str) {
                            Some("user") => Rol::Usuario,
                            Some("assistant") => Rol::Asistente,
                            Some("developer") => Rol::Sistema,
                            otro => {
                                avisar(
                                    &mut avisos,
                                    format!("rol desconocido: {}", otro.unwrap_or("?")),
                                );
                                continue;
                            }
                        };
                        let t = texto_bloques(payload.get("content"));
                        let oculto = rol == Rol::Sistema
                            || (rol == Rol::Usuario && {
                                let ini = t.trim_start();
                                PREFIJOS_OCULTOS.iter().any(|p| ini.starts_with(p))
                            });
                        let tp = if rol == Rol::Sistema {
                            TipoParte::Sistema
                        } else {
                            TipoParte::Texto
                        };
                        (rol, parte(tp, t), oculto)
                    }
                    "reasoning" => {
                        let t = texto_bloques(payload.get("summary"));
                        if t.is_empty() {
                            continue;
                        }
                        (Rol::Asistente, parte(TipoParte::Razonamiento, t), false)
                    }
                    "function_call" | "custom_tool_call" => {
                        let nombre = payload.get("name").and_then(Value::as_str).unwrap_or("?");
                        let args = texto_bloques(payload.get("arguments").or(payload.get("input")));
                        (
                            Rol::Asistente,
                            parte(
                                TipoParte::Herramienta,
                                recortar(format!("{nombre}: {args}")),
                            ),
                            false,
                        )
                    }
                    "function_call_output" | "custom_tool_call_output" => (
                        Rol::Usuario,
                        parte(
                            TipoParte::Herramienta,
                            recortar(texto_bloques(payload.get("output"))),
                        ),
                        false,
                    ),
                    // Sin texto de conversación (catálogo de herramientas, resumen cifrado).
                    "additional_tools" | "tool_search_call" | "tool_search_output"
                    | "compaction" => continue,
                    "web_search_call" => {
                        let q = payload
                            .pointer("/action/query")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if q.is_empty() {
                            continue;
                        }
                        (
                            Rol::Asistente,
                            parte(TipoParte::Herramienta, recortar(format!("web_search: {q}"))),
                            false,
                        )
                    }
                    // Mensaje entre agentes: texto real, pero no lo escribió la persona.
                    "agent_message" => (
                        Rol::Asistente,
                        parte(TipoParte::Texto, texto_bloques(payload.get("content"))),
                        true,
                    ),
                    otro => {
                        avisar(&mut avisos, format!("tipo desconocido: {otro}"));
                        continue;
                    }
                };
                mensajes.push(Mensaje {
                    uuid: None,
                    id_padre: None,
                    rol,
                    modelo: modelo.clone(),
                    hora,
                    oculto,
                    partes,
                });
            }
            t if TIPOS_IGNORADOS.contains(&t) => {}
            otro => avisar(&mut avisos, format!("tipo desconocido: {otro}")),
        }
    }

    let id_original = id_original.ok_or_else(|| "sin session_meta".to_string())?;
    Ok(ConversacionParseada {
        fuente: Fuente::Codex,
        id_original,
        cwd,
        padre_original: None,
        descripcion: None,
        hoja: None,
        mensajes,
        avisos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raiz() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex")
    }
    fn principal() -> ConversacionParseada {
        parsear(&raiz().join("sessions/2026/10/01/rollout-2026-10-01T10-00-00-cx01.jsonl")).unwrap()
    }

    #[test]
    fn parsea_codex() {
        let c = principal();
        assert_eq!(c.fuente, Fuente::Codex);
        assert_eq!(c.id_original, "cx01");
        assert_eq!(c.cwd.as_deref(), Some("/home/u/Proyectos/Mirlo"));
        let visibles: Vec<_> = c
            .mensajes
            .iter()
            .filter(|m| !m.oculto && m.partes.iter().any(|p| p.tipo == TipoParte::Texto))
            .collect();
        assert_eq!(visibles.len(), 2);
        assert_eq!(visibles[0].rol, Rol::Usuario);
        assert_eq!(
            visibles[0].partes[0].contenido,
            "Arregla el parseo de subtítulos en Mirlo"
        );
        assert_eq!(visibles[1].rol, Rol::Asistente);
        assert_eq!(visibles[1].modelo.as_deref(), Some("gpt-6.1-sol"));
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        assert!(c.mensajes.iter().flat_map(|m| &m.partes).any(|p| {
            p.tipo == TipoParte::Herramienta && p.contenido == "exec: grep -n srt src/lib.rs"
        }));
        assert!(c
            .mensajes
            .iter()
            .flat_map(|m| &m.partes)
            .any(|p| p.tipo == TipoParte::Herramienta && p.contenido == "42:fn parse_srt()"));
    }

    #[test]
    fn oculta_agents_md() {
        let c = principal();
        let m = c
            .mensajes
            .iter()
            .find(|m| {
                m.partes
                    .iter()
                    .any(|p| p.contenido.starts_with("# AGENTS.md"))
            })
            .unwrap();
        assert!(m.oculto);
        let d = c.mensajes.iter().find(|m| m.rol == Rol::Sistema).unwrap();
        assert!(d.oculto);
    }

    #[test]
    fn razonamiento_con_summary() {
        let c = principal();
        let r: Vec<_> = c
            .mensajes
            .iter()
            .flat_map(|m| &m.partes)
            .filter(|p| p.tipo == TipoParte::Razonamiento)
            .collect();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].contenido, "Pienso en el SRT");
    }

    #[test]
    fn sin_session_meta_es_error() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        std::fs::write(
            &ruta,
            r#"{"type":"event_msg","payload":{"type":"x"}}"#.to_string() + "\n",
        )
        .unwrap();
        assert_eq!(parsear(&ruta).unwrap_err(), "sin session_meta");
    }

    #[test]
    fn json_roto_en_medio_es_error() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        std::fs::write(&ruta, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"a\"}}\n{roto\n{\"type\":\"event_msg\"}\n").unwrap();
        assert!(parsear(&ruta).unwrap_err().contains("línea 2"));
        // Última línea rota: se ignora.
        std::fs::write(
            &ruta,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"a\"}}\n{roto\n",
        )
        .unwrap();
        assert_eq!(parsear(&ruta).unwrap().id_original, "a");
    }

    #[test]
    fn avisa_tipos_desconocidos_una_vez() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        let t = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"a\"}}\n",
            "{\"type\":\"raro\"}\n{\"type\":\"raro\"}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"additional_tools\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"otro_item\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{}}\n",
        );
        std::fs::write(&ruta, t).unwrap();
        assert_eq!(
            parsear(&ruta).unwrap().avisos,
            vec!["tipo desconocido: raro", "tipo desconocido: otro_item"]
        );
    }

    #[test]
    fn function_call_y_recorte() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        let grande = "y".repeat(100_000);
        let t = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"a\"}}}}\n{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call\",\"name\":\"sh\",\"arguments\":\"{{}}\"}}}}\n{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call_output\",\"output\":\"{grande}\"}}}}\n"
        );
        std::fs::write(&ruta, t).unwrap();
        let c = parsear(&ruta).unwrap();
        assert_eq!(c.mensajes[0].rol, Rol::Asistente);
        assert_eq!(c.mensajes[0].partes[0].contenido, "sh: {}");
        assert_eq!(c.mensajes[1].rol, Rol::Usuario);
        assert!(c.mensajes[1].partes[0]
            .contenido
            .ends_with("\n…[recortado]"));
    }

    #[test]
    fn ignora_metadatos_y_mapea_agent_message() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        let t = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"a\"}}\n",
            "{\"type\":\"inter_agent_communication_metadata\",\"payload\":{\"trigger_turn\":true}}\n",
            "{\"type\":\"compacted\",\"payload\":{\"message\":\"\",\"replacement_history\":[]}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"agent_message\",\"author\":\"/root/x\",\"content\":[{\"type\":\"input_text\",\"text\":\"informe del subagente\"}]}}\n",
        );
        std::fs::write(&ruta, t).unwrap();
        let c = parsear(&ruta).unwrap();
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        assert_eq!(c.mensajes.len(), 1);
        assert!(c.mensajes[0].oculto);
        assert_eq!(c.mensajes[0].partes[0].contenido, "informe del subagente");
    }

    #[test]
    fn mapea_web_search_e_ignora_items_sin_texto() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        let t = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"a\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"web_search_call\",\"action\":{\"type\":\"search\",\"query\":\"consulta inventada\"}}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"tool_search_call\",\"arguments\":{}}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"tool_search_output\",\"tools\":[]}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"compaction\",\"encrypted_content\":\"xx\"}}\n",
        );
        std::fs::write(&ruta, t).unwrap();
        let c = parsear(&ruta).unwrap();
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        assert_eq!(c.mensajes.len(), 1);
        assert_eq!(
            c.mensajes[0].partes[0].contenido,
            "web_search: consulta inventada"
        );
    }

    #[test]
    fn oculta_inyecciones_recurrentes() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("rollout-x.jsonl");
        let mut t = String::from("{\"type\":\"session_meta\",\"payload\":{\"id\":\"a\"}}\n");
        for inicio in [
            "<turn_aborted>\nAviso inventado",
            "<recommended_plugins>\nLista inventada",
            "<subagent_notification>\n{}",
            "<system-reminder>\nNota inventada",
            "<user_instructions>\nReglas inventadas",
            "<command-name>/compact</command-name>",
            "This session is being continued from otra",
            "[Request interrupted by user]",
            "Texto normal de la persona",
        ] {
            t += &format!(
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":{}}}]}}}}\n",
                serde_json::to_string(inicio).unwrap()
            );
        }
        std::fs::write(&ruta, t).unwrap();
        let c = parsear(&ruta).unwrap();
        let visibles: Vec<_> = c.mensajes.iter().filter(|m| !m.oculto).collect();
        assert_eq!(visibles.len(), 1);
        assert_eq!(
            visibles[0].partes[0].contenido,
            "Texto normal de la persona"
        );
    }

    #[test]
    fn lee_rollout_comprimido_con_zstd() {
        let d = tempfile::tempdir().unwrap();
        let plano = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"zz9\",\"cwd\":\"/home/u/Z\"}}\n\
            {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hola comprimido\"}]}}\n";
        let zst = ruzstd::encoding::compress_to_vec(
            plano.as_bytes(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let ruta = d
            .path()
            .join("sessions/2026/01/01/rollout-2026-01-01T00-00-00-zz9.jsonl.zst");
        std::fs::create_dir_all(ruta.parent().unwrap()).unwrap();
        std::fs::write(&ruta, zst).unwrap();
        assert_eq!(descubrir(d.path()), vec![ruta.clone()]);
        let c = parsear(&ruta).unwrap();
        assert_eq!(c.id_original, "zz9");
        assert_eq!(c.mensajes[0].partes[0].contenido, "hola comprimido");
    }

    #[test]
    fn descubrir_ambas_carpetas() {
        assert_eq!(descubrir(&raiz()).len(), 2);
    }
}
