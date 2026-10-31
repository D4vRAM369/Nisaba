//! Adaptador de exportaciones de Claude.ai (`conversations.json`, `projects`, `memories`,
//! `frames` y `design_chats`). Nunca lee `light_metadata`.

use std::collections::{HashMap, HashSet};

use serde_json::value::RawValue;
use serde_json::Value;

use super::zip::{recorrer_array, Lector};
use super::{Formato, Salida};
use crate::modelo::{
    ConversacionImportada, ConversacionParseada, Fuente, Mensaje, Parte, Rol, TipoParte, TipoWeb,
};

pub(crate) const FORMATO: Formato = Formato {
    nombre: "claude-web",
    reconoce,
    leer,
};

const PADRE_RAIZ: &str = "00000000-0000-4000-8000-000000000000";

const CAMPOS_CONVERSACION: &[&str] = &[
    "uuid",
    "name",
    "summary",
    "model",
    "created_at",
    "updated_at",
    "account",
    "chat_messages",
    "computer_folders",
    "project",
    "project_uuid",
    "is_starred",
    "current_leaf_message_uuid",
    "settings",
];

const CAMPOS_MENSAJE: &[&str] = &[
    "uuid",
    "text",
    "content",
    "sender",
    "index",
    "created_at",
    "updated_at",
    "edited_at",
    "chat_feedback",
    "attachments",
    "files",
    "files_v2",
    "sync_sources",
    "parent_message_uuid",
    "model",
    "stop_reason",
];

fn reconoce(lector: &mut Lector) -> bool {
    let nombres = lector.nombres();
    if nombres
        .iter()
        .any(|n| n == "chat.html" || n == "users.json")
    {
        return false;
    }
    if nombres.iter().any(|n| {
        (n.starts_with("projects/") && n.ends_with(".json"))
            || (n.starts_with("memories/") && n.ends_with(".json"))
            || (n.starts_with("design_chats/") && n.ends_with(".json"))
            || (n.starts_with("artifacts/") && n.ends_with("/artifact.json"))
    }) {
        return true;
    }
    if nombres.iter().any(|n| n == "conversations.json") {
        let Some(bytes) = lector.muestra("conversations.json", 65_536) else {
            return false;
        };
        let muestra = String::from_utf8_lossy(&bytes);
        let recortada = muestra.trim();
        if muestra.contains("\"mapping\"") || muestra.contains("\"current_node\"") {
            return false;
        }
        if muestra.contains("\"chat_messages\"") || recortada == "[]" {
            return true;
        }
    }
    false
}

fn leer(lector: &mut Lector, salida: Salida<'_>, avisos: &mut Vec<String>) -> Result<(), String> {
    let nombres = lector.nombres();
    let mut proyectos = HashMap::new();

    let mut entradas_proyectos: Vec<_> = nombres
        .iter()
        .filter(|n| n.starts_with("projects/") && n.ends_with(".json"))
        .cloned()
        .collect();
    entradas_proyectos.sort();
    for entrada in entradas_proyectos {
        leer_proyecto(lector, &entrada, &mut proyectos, salida)?;
    }

    let mut entradas_memorias: Vec<_> = nombres
        .iter()
        .filter(|n| n.starts_with("memories/") && n.ends_with(".json"))
        .cloned()
        .collect();
    entradas_memorias.sort();
    for entrada in entradas_memorias {
        leer_memoria(lector, &entrada, salida)?;
    }

    let mut entradas_frames: Vec<_> = nombres
        .iter()
        .filter(|n| n.starts_with("artifacts/") && n.ends_with("/artifact.json"))
        .cloned()
        .collect();
    entradas_frames.sort();
    for entrada in entradas_frames {
        leer_frame(lector, &entrada, &nombres, salida)?;
    }

    let mut entradas_diseno: Vec<_> = nombres
        .iter()
        .filter(|n| n.starts_with("design_chats/") && n.ends_with(".json"))
        .cloned()
        .collect();
    entradas_diseno.sort();
    for entrada in entradas_diseno {
        leer_diseno(lector, &entrada, &proyectos, salida)?;
    }

    if nombres.iter().any(|n| n == "conversations.json") {
        leer_conversaciones(lector, &proyectos, salida, avisos)?;
    }

    Ok(())
}

fn texto_no_vacio(v: &Value, clave: &str) -> Option<String> {
    v.get(clave)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn leer_proyecto(
    lector: &mut Lector,
    entrada: &str,
    proyectos: &mut HashMap<String, String>,
    salida: Salida<'_>,
) -> Result<(), String> {
    let raw = lector.leer_cadena(entrada)?;
    let v: Value =
        serde_json::from_str(&raw).map_err(|_| "JSON de proyecto no válido".to_string())?;
    let Some(uuid) = texto_no_vacio(&v, "uuid") else {
        return Ok(());
    };
    let nombre = texto_no_vacio(&v, "name").unwrap_or_else(|| uuid.clone());
    proyectos.insert(uuid.clone(), nombre.clone());

    let inicio = texto_no_vacio(&v, "created_at");
    let fin = texto_no_vacio(&v, "updated_at").or_else(|| inicio.clone());
    let docs = v.get("docs").and_then(Value::as_array);
    let mut emitidos = 0;

    if let Some(docs) = docs {
        for (i, doc) in docs.iter().enumerate() {
            let doc_uuid = texto_no_vacio(doc, "uuid").unwrap_or_else(|| i.to_string());
            let titulo = texto_no_vacio(doc, "filename").or_else(|| Some(nombre.clone()));
            let contenido = doc
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let doc_inicio = texto_no_vacio(doc, "created_at").or_else(|| inicio.clone());
            let doc_fin = fin.clone().or_else(|| doc_inicio.clone());
            salida(ConversacionImportada {
                base: ConversacionParseada {
                    fuente: Fuente::ClaudeWeb,
                    id_original: format!("project-{uuid}:{doc_uuid}"),
                    cwd: None,
                    padre_original: None,
                    descripcion: None,
                    hoja: None,
                    mensajes: vec![Mensaje {
                        uuid: Some(doc_uuid),
                        id_padre: None,
                        rol: Rol::Usuario,
                        modelo: None,
                        hora: doc_inicio.clone(),
                        oculto: false,
                        partes: vec![Parte {
                            tipo: TipoParte::Texto,
                            contenido,
                        }],
                    }],
                    avisos: vec![],
                },
                titulo,
                proyecto: Some(nombre.clone()),
                inicio: doc_inicio,
                fin: doc_fin,
                tipo: TipoWeb::Documento,
                original: Some(raw.clone()),
            })?;
            emitidos += 1;
        }
    }

    let desc = texto_no_vacio(&v, "description");
    let prompt = texto_no_vacio(&v, "prompt_template");
    if emitidos == 0 || desc.is_some() || prompt.is_some() {
        let mut secciones = vec![];
        if let Some(d) = desc {
            secciones.push(d);
        }
        if let Some(p) = prompt {
            secciones.push(p);
        }
        if secciones.is_empty() {
            secciones.push(nombre.clone());
        }
        salida(ConversacionImportada {
            base: ConversacionParseada {
                fuente: Fuente::ClaudeWeb,
                id_original: format!("project-{uuid}"),
                cwd: None,
                padre_original: None,
                descripcion: None,
                hoja: None,
                mensajes: vec![Mensaje {
                    uuid: Some(uuid),
                    id_padre: None,
                    rol: Rol::Usuario,
                    modelo: None,
                    hora: inicio.clone(),
                    oculto: false,
                    partes: vec![Parte {
                        tipo: TipoParte::Texto,
                        contenido: secciones.join("\n\n"),
                    }],
                }],
                avisos: vec![],
            },
            titulo: Some(nombre.clone()),
            proyecto: Some(nombre),
            inicio,
            fin,
            tipo: TipoWeb::Documento,
            original: Some(raw),
        })?;
    }

    Ok(())
}

fn leer_memoria(lector: &mut Lector, entrada: &str, salida: Salida<'_>) -> Result<(), String> {
    let raw = lector.leer_cadena(entrada)?;
    let v: Value =
        serde_json::from_str(&raw).map_err(|_| "JSON de memoria no válido".to_string())?;
    let base_id = texto_no_vacio(&v, "account_uuid").unwrap_or_else(|| {
        entrada
            .trim_start_matches("memories/")
            .trim_end_matches(".json")
            .to_string()
    });

    let mut emitidos = 0;
    if let Some(archivos) = v.get("memory_files").and_then(Value::as_array) {
        for (i, arch) in archivos.iter().enumerate() {
            let ruta = texto_no_vacio(arch, "path").unwrap_or_else(|| format!("memoria-{i}"));
            let contenido = arch
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let fin = texto_no_vacio(arch, "updated_at");
            salida(ConversacionImportada {
                base: ConversacionParseada {
                    fuente: Fuente::ClaudeWeb,
                    id_original: format!("memory-{base_id}:{ruta}"),
                    cwd: None,
                    padre_original: None,
                    descripcion: None,
                    hoja: None,
                    mensajes: vec![Mensaje {
                        uuid: None,
                        id_padre: None,
                        rol: Rol::Usuario,
                        modelo: None,
                        hora: fin.clone(),
                        oculto: false,
                        partes: vec![Parte {
                            tipo: TipoParte::Texto,
                            contenido,
                        }],
                    }],
                    avisos: vec![],
                },
                titulo: Some(ruta),
                proyecto: None,
                inicio: fin.clone(),
                fin,
                tipo: TipoWeb::Documento,
                original: Some(raw.clone()),
            })?;
            emitidos += 1;
        }
    }

    let mem_conv = texto_no_vacio(&v, "conversations_memory");
    if emitidos == 0 || mem_conv.is_some() {
        let contenido = mem_conv.unwrap_or_else(|| raw.clone());
        salida(ConversacionImportada {
            base: ConversacionParseada {
                fuente: Fuente::ClaudeWeb,
                id_original: format!("memory-{base_id}"),
                cwd: None,
                padre_original: None,
                descripcion: None,
                hoja: None,
                mensajes: vec![Mensaje {
                    uuid: None,
                    id_padre: None,
                    rol: Rol::Usuario,
                    modelo: None,
                    hora: None,
                    oculto: false,
                    partes: vec![Parte {
                        tipo: TipoParte::Texto,
                        contenido,
                    }],
                }],
                avisos: vec![],
            },
            titulo: Some("Memoria de Claude.ai".to_string()),
            proyecto: None,
            inicio: None,
            fin: None,
            tipo: TipoWeb::Documento,
            original: Some(raw),
        })?;
    }

    Ok(())
}

fn leer_frame(
    lector: &mut Lector,
    entrada: &str,
    nombres: &[String],
    salida: Salida<'_>,
) -> Result<(), String> {
    let raw = lector.leer_cadena(entrada)?;
    let mut v: Value =
        serde_json::from_str(&raw).map_err(|_| "JSON de frame no válido".to_string())?;
    let carpeta = entrada.trim_end_matches("artifact.json");
    let id = texto_no_vacio(&v, "id").unwrap_or_else(|| {
        carpeta
            .trim_start_matches("artifacts/")
            .trim_end_matches('/')
            .to_string()
    });
    let fin = texto_no_vacio(&v, "updated_at");
    let activa = texto_no_vacio(&v, "active_version");

    let mut titulo = None;
    let mut inicio = None;
    if let Some(versiones) = v.get("versions").and_then(Value::as_array) {
        if let Some(primera) = versiones.first() {
            inicio = texto_no_vacio(primera, "created_at");
        }
        for ver in versiones {
            let es_activa = activa
                .as_deref()
                .is_some_and(|a| ver.get("id").and_then(Value::as_str) == Some(a));
            if es_activa || titulo.is_none() {
                if let Some(t) = texto_no_vacio(ver, "title") {
                    titulo = Some(t);
                }
            }
        }
    }

    let ruta_md = format!("{carpeta}page.md");
    let texto_md = if nombres.iter().any(|n| n == &ruta_md) {
        Some(lector.leer_cadena(&ruta_md)?)
    } else {
        None
    };

    let prefijo_ver = format!("{carpeta}versions/");
    let ruta_html = activa
        .as_ref()
        .map(|a| format!("{prefijo_ver}{a}.html"))
        .filter(|r| nombres.iter().any(|n| n == r))
        .or_else(|| {
            nombres
                .iter()
                .find(|n| n.starts_with(&prefijo_ver) && n.ends_with(".html"))
                .cloned()
        });
    let texto_html = match ruta_html {
        Some(r) => Some(lector.leer_cadena(&r)?),
        None => None,
    };

    if let (Some(html), Some(obj)) = (&texto_html, v.as_object_mut()) {
        obj.insert("html".to_string(), Value::String(html.clone()));
    }
    let original = serde_json::to_string(&v).unwrap_or(raw);
    let contenido = texto_md
        .or(texto_html)
        .or_else(|| titulo.clone())
        .unwrap_or_else(|| id.clone());

    salida(ConversacionImportada {
        base: ConversacionParseada {
            fuente: Fuente::ClaudeWeb,
            id_original: format!("frame-{id}"),
            cwd: None,
            padre_original: None,
            descripcion: None,
            hoja: None,
            mensajes: vec![Mensaje {
                uuid: Some(id.clone()),
                id_padre: None,
                rol: Rol::Asistente,
                modelo: None,
                hora: fin.clone().or_else(|| inicio.clone()),
                oculto: false,
                partes: vec![Parte {
                    tipo: TipoParte::Texto,
                    contenido,
                }],
            }],
            avisos: vec![],
        },
        titulo: titulo.or(Some(id)),
        proyecto: None,
        inicio,
        fin,
        tipo: TipoWeb::Documento,
        original: Some(original),
    })
}

fn leer_diseno(
    lector: &mut Lector,
    entrada: &str,
    proyectos: &HashMap<String, String>,
    salida: Salida<'_>,
) -> Result<(), String> {
    let raw = lector.leer_cadena(entrada)?;
    let v: Value =
        serde_json::from_str(&raw).map_err(|_| "JSON de diseño no válido".to_string())?;
    let Some(uuid) = texto_no_vacio(&v, "uuid") else {
        return Ok(());
    };
    let titulo = texto_no_vacio(&v, "title");
    let inicio = texto_no_vacio(&v, "created_at");
    let fin = texto_no_vacio(&v, "updated_at").or_else(|| inicio.clone());

    let proyecto = v.get("project").and_then(|p| {
        texto_no_vacio(p, "name").or_else(|| {
            texto_no_vacio(p, "uuid").map(|u| {
                proyectos
                    .get(&u)
                    .cloned()
                    .unwrap_or_else(|| format!("uuid:{u}"))
            })
        })
    });

    let mut mensajes = vec![];
    if let Some(lista) = v.get("messages").and_then(Value::as_array) {
        for m in lista {
            let m_uuid = texto_no_vacio(m, "uuid");
            let hora = texto_no_vacio(m, "created_at");
            let rol = match m.get("role").and_then(Value::as_str) {
                Some("assistant") => Rol::Asistente,
                _ => Rol::Usuario,
            };
            let mut partes = vec![];
            if let Some(cont) = m.get("content") {
                if let Some(t) = texto_no_vacio(cont, "content") {
                    partes.push(Parte {
                        tipo: TipoParte::Texto,
                        contenido: t,
                    });
                }
                if let Some(adjs) = cont.get("attachments").and_then(Value::as_array) {
                    for adj in adjs {
                        if let Some(c) = texto_no_vacio(adj, "content") {
                            let nombre = texto_no_vacio(adj, "name");
                            let texto = match nombre {
                                Some(n) => format!("[Adjunto: {n}]\n{c}"),
                                None => c,
                            };
                            partes.push(Parte {
                                tipo: TipoParte::Texto,
                                contenido: texto,
                            });
                        }
                    }
                }
            }
            if !partes.is_empty() {
                mensajes.push(Mensaje {
                    uuid: m_uuid,
                    id_padre: None,
                    rol,
                    modelo: None,
                    hora,
                    oculto: false,
                    partes,
                });
            }
        }
    }

    salida(ConversacionImportada {
        base: ConversacionParseada {
            fuente: Fuente::ClaudeWeb,
            id_original: uuid,
            cwd: None,
            padre_original: None,
            descripcion: None,
            hoja: None,
            mensajes,
            avisos: vec![],
        },
        titulo,
        proyecto,
        inicio,
        fin,
        tipo: TipoWeb::Diseno,
        original: Some(raw),
    })
}

fn leer_conversaciones(
    lector: &mut Lector,
    proyectos: &HashMap<String, String>,
    salida: Salida<'_>,
    avisos_formato: &mut Vec<String>,
) -> Result<(), String> {
    let r = lector.entrada("conversations.json")?;
    recorrer_array(r, |raw: Box<RawValue>| {
        let json_str = raw.get().to_string();
        let v: Value = serde_json::from_str(&json_str)
            .map_err(|_| "conversación JSON no válida".to_string())?;
        let Some(conv) = parsear_conversacion(&v, json_str, proyectos, avisos_formato) else {
            return Ok(());
        };
        salida(conv)
    })
}

fn parsear_conversacion(
    v: &Value,
    original: String,
    proyectos: &HashMap<String, String>,
    avisos_formato: &mut Vec<String>,
) -> Option<ConversacionImportada> {
    let obj = v.as_object()?;
    let uuid = texto_no_vacio(v, "uuid")?;

    let mut avisos = vec![];
    for clave in obj.keys() {
        if !CAMPOS_CONVERSACION.contains(&clave.as_str()) {
            avisos.push("campo desconocido en conversación de Claude.ai".to_string());
        }
    }

    let inicio = texto_no_vacio(v, "created_at");
    let fin = texto_no_vacio(v, "updated_at").or_else(|| inicio.clone());
    let modelo_conv = texto_no_vacio(v, "model");

    let mut proyecto = v
        .get("project")
        .and_then(|p| {
            if let Some(s) = p.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                return Some(s.to_string());
            }
            texto_no_vacio(p, "name").or_else(|| {
                texto_no_vacio(p, "uuid").map(|u| {
                    proyectos
                        .get(&u)
                        .cloned()
                        .unwrap_or_else(|| format!("uuid:{u}"))
                })
            })
        })
        .or_else(|| {
            texto_no_vacio(v, "project_uuid").map(|u| {
                proyectos
                    .get(&u)
                    .cloned()
                    .unwrap_or_else(|| format!("uuid:{u}"))
            })
        });

    let lista_msgs = v
        .get("chat_messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let uuids_presentes: HashSet<String> = lista_msgs
        .iter()
        .filter_map(|m| texto_no_vacio(m, "uuid"))
        .collect();

    let mut mensajes = Vec::with_capacity(lista_msgs.len());
    let mut orden_horas: Vec<(String, String, usize)> = Vec::new();
    let mut cita_num = 0usize;

    for (idx, m) in lista_msgs.iter().enumerate() {
        if let Some(m_obj) = m.as_object() {
            for clave in m_obj.keys() {
                if !CAMPOS_MENSAJE.contains(&clave.as_str()) {
                    avisos.push("campo desconocido en mensaje de Claude.ai".to_string());
                }
            }
        }

        if proyecto.is_none() {
            if let Some(u) = buscar_uuid_proyecto_en_mensaje(m) {
                proyecto = Some(
                    proyectos
                        .get(&u)
                        .cloned()
                        .unwrap_or_else(|| format!("uuid:{u}")),
                );
            }
        }

        let m_uuid = texto_no_vacio(m, "uuid");
        let id_padre = texto_no_vacio(m, "parent_message_uuid")
            .filter(|p| p != PADRE_RAIZ && uuids_presentes.contains(p));
        let rol = match m.get("sender").and_then(Value::as_str) {
            Some("assistant") => Rol::Asistente,
            _ => Rol::Usuario,
        };
        let hora = texto_no_vacio(m, "created_at");
        let hora_act = texto_no_vacio(m, "updated_at")
            .or_else(|| hora.clone())
            .unwrap_or_default();
        if let Some(ref u) = m_uuid {
            orden_horas.push((u.clone(), hora_act, idx));
        }

        let modelo = texto_no_vacio(m, "model").or_else(|| {
            if rol == Rol::Asistente {
                modelo_conv.clone()
            } else {
                None
            }
        });

        let mut partes = extraer_partes(m, &mut cita_num, &mut avisos);
        extraer_adjuntos(m, &mut partes);

        mensajes.push(Mensaje {
            uuid: m_uuid,
            id_padre,
            rol,
            modelo,
            hora,
            oculto: false,
            partes,
        });
    }

    let padres: HashSet<&str> = mensajes
        .iter()
        .filter_map(|m| m.id_padre.as_deref())
        .collect();

    let hoja = orden_horas
        .iter()
        .filter(|(u, _, _)| !padres.contains(u.as_str()))
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)))
        .map(|(u, _, _)| u.clone());

    let titulo = texto_no_vacio(v, "name").or_else(|| texto_no_vacio(v, "summary"));

    for a in &avisos {
        if !avisos_formato.contains(a) {
            avisos_formato.push(a.clone());
        }
    }

    Some(ConversacionImportada {
        base: ConversacionParseada {
            fuente: Fuente::ClaudeWeb,
            id_original: uuid,
            cwd: None,
            padre_original: None,
            descripcion: None,
            hoja,
            mensajes,
            avisos,
        },
        titulo,
        proyecto,
        inicio,
        fin,
        tipo: TipoWeb::Conversacion,
        original: Some(original),
    })
}

fn extraer_partes(m: &Value, cita_num: &mut usize, avisos: &mut Vec<String>) -> Vec<Parte> {
    let mut partes = vec![];
    let mut vio_bloque_tecnico = false;

    if let Some(bloques) = m.get("content").and_then(Value::as_array) {
        for b in bloques {
            let tipo = b.get("type").and_then(Value::as_str).unwrap_or("");
            match tipo {
                "text" => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        if !t.trim().is_empty() {
                            let marcado = marcar_citas(t, b.get("citations"), cita_num);
                            partes.push(Parte {
                                tipo: TipoParte::Texto,
                                contenido: marcado,
                            });
                        }
                    }
                }
                "thinking" => {
                    if let Some(t) = texto_no_vacio(b, "thinking") {
                        partes.push(Parte {
                            tipo: TipoParte::Razonamiento,
                            contenido: t,
                        });
                    }
                }
                "tool_use" => {
                    let nombre =
                        texto_no_vacio(b, "name").unwrap_or_else(|| "herramienta".to_string());
                    let entrada = b
                        .get("input")
                        .map(|i| serde_json::to_string(i).unwrap_or_default())
                        .unwrap_or_else(|| "{}".to_string());
                    partes.push(Parte {
                        tipo: TipoParte::Herramienta,
                        contenido: format!("{nombre}: {entrada}"),
                    });
                }
                "tool_result" => {
                    let nombre =
                        texto_no_vacio(b, "name").unwrap_or_else(|| "resultado".to_string());
                    let salida = texto_de_resultado_herramienta(b);
                    if !salida.is_empty() {
                        if let Some(ultima) = partes.last_mut().filter(|p| {
                            p.tipo == TipoParte::Herramienta
                                && !p.contenido.contains("\nResultado: ")
                        }) {
                            ultima.contenido.push_str(&format!("\nResultado: {salida}"));
                        } else {
                            partes.push(Parte {
                                tipo: TipoParte::Herramienta,
                                contenido: format!("{nombre}: {{}}\nResultado: {salida}"),
                            });
                        }
                    }
                }
                "image" => {
                    partes.push(Parte {
                        tipo: TipoParte::Imagen,
                        contenido: "[Imagen adjunta (no incluida en la exportación de Claude.ai)]"
                            .to_string(),
                    });
                }
                "document" => {
                    let titulo =
                        texto_no_vacio(b, "title").unwrap_or_else(|| "Documento".to_string());
                    partes.push(Parte {
                        tipo: TipoParte::Texto,
                        contenido: format!("[Documento: {titulo}]"),
                    });
                }
                "voice_note" => {
                    if let Some(t) =
                        texto_no_vacio(b, "text").or_else(|| texto_no_vacio(b, "title"))
                    {
                        partes.push(Parte {
                            tipo: TipoParte::Texto,
                            contenido: t,
                        });
                    }
                }
                "injected_prompt_block" | "token_budget" | "flag" => {
                    vio_bloque_tecnico = true;
                }
                _ => {
                    avisos.push("parte desconocida en conversación de Claude.ai".to_string());
                }
            }
        }
    }

    if partes.is_empty() && !vio_bloque_tecnico {
        if let Some(t) = texto_no_vacio(m, "text") {
            partes.push(Parte {
                tipo: TipoParte::Texto,
                contenido: t,
            });
        }
    }

    partes
}

fn marcar_citas(texto: &str, citations: Option<&Value>, cita_num: &mut usize) -> String {
    let Some(arr) = citations.and_then(Value::as_array) else {
        return texto.to_string();
    };
    let mut marcas = vec![];
    for c in arr {
        let url = c
            .get("details")
            .and_then(|d| d.get("url"))
            .and_then(Value::as_str)
            .or_else(|| c.get("url").and_then(Value::as_str));
        if url.is_some_and(|u| !u.is_empty()) {
            *cita_num += 1;
            marcas.push(format!("[{cita_num}]"));
        }
    }
    if marcas.is_empty() {
        texto.to_string()
    } else {
        format!("{texto} {}", marcas.join(""))
    }
}

fn texto_de_resultado_herramienta(b: &Value) -> String {
    let Some(cont) = b.get("content") else {
        return String::new();
    };
    if let Some(s) = cont.as_str() {
        return s.trim().to_string();
    }
    if let Some(arr) = cont.as_array() {
        let textos: Vec<&str> = arr
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if !textos.is_empty() {
            return textos.join("\n");
        }
    }
    serde_json::to_string(cont).unwrap_or_default()
}

fn extraer_adjuntos(m: &Value, partes: &mut Vec<Parte>) {
    if let Some(adjs) = m.get("attachments").and_then(Value::as_array) {
        for adj in adjs {
            let nombre = texto_no_vacio(adj, "file_name");
            let extraido = texto_no_vacio(adj, "extracted_content");
            match (nombre, extraido) {
                (Some(n), Some(c)) => partes.push(Parte {
                    tipo: TipoParte::Texto,
                    contenido: format!("[Adjunto: {n}]\n{c}"),
                }),
                (None, Some(c)) => partes.push(Parte {
                    tipo: TipoParte::Texto,
                    contenido: c,
                }),
                (Some(n), None) => partes.push(Parte {
                    tipo: TipoParte::Texto,
                    contenido: format!("[Adjunto: {n}]"),
                }),
                (None, None) => {}
            }
        }
    }
}

fn buscar_uuid_proyecto_en_mensaje(m: &Value) -> Option<String> {
    let bloques = m.get("content")?.as_array()?;
    for b in bloques {
        if let Some(p) = b.get("prompt").and_then(Value::as_str) {
            if let Some(u) = extraer_uuid_tras_projects(p) {
                return Some(u);
            }
        }
        if let Some(ruta) = b
            .get("input")
            .and_then(|i| i.get("path"))
            .and_then(Value::as_str)
        {
            if let Some(u) = extraer_uuid_tras_projects(ruta) {
                return Some(u);
            }
        }
    }
    None
}

fn extraer_uuid_tras_projects(texto: &str) -> Option<String> {
    let pos = texto.find("/projects/")?;
    let resto = &texto[pos + "/projects/".len()..];
    let candidato: String = resto
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    if candidato.len() >= 8 {
        Some(candidato)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archivo_web;
    use crate::boveda::LlaveMaestra;
    use crate::busqueda::{buscar, Filtros};
    use crate::importar::{importar, reconocer};
    use crate::lector::{detalles_web, leer, Bloque, Tramo};
    use ::zip::write::SimpleFileOptions;
    use ::zip::{CompressionMethod, ZipWriter};
    use serde_json::json;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use tempfile::{tempdir, TempDir};

    fn base() -> (TempDir, rusqlite::Connection) {
        let dir = tempdir().unwrap();
        let con =
            archivo_web::abrir(&dir.path().join("casa"), &LlaveMaestra::de_prueba(70)).unwrap();
        (dir, con)
    }

    fn escribir_zip(ruta: &Path, entradas: &[(&str, serde_json::Value)]) {
        let mut z = ZipWriter::new(std::fs::File::create(ruta).unwrap());
        let op = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (nombre, valor) in entradas {
            z.start_file(*nombre, op).unwrap();
            let bytes = match valor {
                Value::String(s) if !nombre.ends_with(".json") => s.as_bytes().to_vec(),
                _ => serde_json::to_vec(valor).unwrap(),
            };
            z.write_all(&bytes).unwrap();
        }
        z.finish().unwrap();
    }

    fn zip_conversaciones(dir: &Path, convs: Value) -> PathBuf {
        let ruta = dir.path().join("conversations-000.zip");
        escribir_zip(&ruta, &[("conversations.json", convs)]);
        ruta
    }

    trait PathExt {
        fn path(&self) -> &Path;
    }
    impl PathExt for Path {
        fn path(&self) -> &Path {
            self
        }
    }

    #[test]
    fn lee_conversacion_basica() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-1",
                "name": "Cultivo de tomates",
                "summary": "",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:05:00Z",
                "account": { "uuid": "acc-1" },
                "chat_messages": [
                    {
                        "uuid": "m-1",
                        "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
                        "sender": "human",
                        "created_at": "2026-03-01T10:00:00Z",
                        "updated_at": "2026-03-01T10:00:00Z",
                        "text": "¿Cuándo sembrar tomates?",
                        "content": [{ "type": "text", "text": "¿Cuándo sembrar tomates?" }],
                        "attachments": [],
                        "files": []
                    },
                    {
                        "uuid": "m-2",
                        "parent_message_uuid": "m-1",
                        "sender": "assistant",
                        "created_at": "2026-03-01T10:01:00Z",
                        "updated_at": "2026-03-01T10:01:00Z",
                        "text": "En primavera, tras las heladas.",
                        "content": [{ "type": "text", "text": "En primavera, tras las heladas." }],
                        "attachments": [],
                        "files": []
                    }
                ]
            }]),
        );

        assert_eq!(reconocer(&zip).unwrap(), "claude-web");
        let r = importar(&mut con, &zip).unwrap();
        assert_eq!(r.nuevas, 1);

        let res = buscar(&con, "heladas", &Filtros::default(), "cli").unwrap();
        assert_eq!(res.resultados.len(), 1);
        assert_eq!(res.resultados[0].id_publico, "cw-conv-1");
        assert_eq!(res.resultados[0].titulo, "Cultivo de tomates");

        let lec = leer(&con, 1, &Tramo::default()).unwrap();
        assert_eq!(lec.bloques.len(), 2);
    }

    #[test]
    fn rama_mas_reciente_es_la_hoja() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-ramas",
                "name": "Prueba de ramas",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:10:00Z",
                "chat_messages": [
                    {
                        "uuid": "raiz",
                        "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
                        "sender": "human",
                        "created_at": "2026-03-01T10:00:00Z",
                        "updated_at": "2026-03-01T10:00:00Z",
                        "content": [{ "type": "text", "text": "Pregunta inicial" }]
                    },
                    {
                        "uuid": "rama-vieja",
                        "parent_message_uuid": "raiz",
                        "sender": "assistant",
                        "created_at": "2026-03-01T10:01:00Z",
                        "updated_at": "2026-03-01T10:01:00Z",
                        "content": [{ "type": "text", "text": "Respuesta descartada antigua" }]
                    },
                    {
                        "uuid": "rama-nueva",
                        "parent_message_uuid": "raiz",
                        "sender": "assistant",
                        "created_at": "2026-03-01T10:05:00Z",
                        "updated_at": "2026-03-01T10:05:00Z",
                        "content": [{ "type": "text", "text": "Respuesta definitiva reciente" }]
                    }
                ]
            }]),
        );

        importar(&mut con, &zip).unwrap();
        let lec = leer(&con, 1, &Tramo::default()).unwrap();
        let textos: Vec<&str> = lec
            .bloques
            .iter()
            .filter_map(|b| match b {
                Bloque::Usuario { texto, .. } | Bloque::Asistente { texto, .. } => {
                    Some(texto.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            textos,
            vec!["Pregunta inicial", "Respuesta definitiva reciente"]
        );

        let det = detalles_web(&con, 1).unwrap();
        assert_eq!(det.ramas.len(), 2);
        assert!(det.ramas.iter().any(|r| r.hoja == "rama-nueva" && r.activa));
        assert!(det
            .ramas
            .iter()
            .any(|r| r.hoja == "rama-vieja" && !r.activa));
    }

    #[test]
    fn proyecto_se_asocia() {
        let (dir, mut con) = base();
        let carpeta = dir.path().join("exportacion");
        std::fs::create_dir_all(&carpeta).unwrap();

        escribir_zip(
            &carpeta.join("projects-000.zip"),
            &[(
                "projects/0199e2d2-aaaa-bbbb-cccc-111122223333.json",
                json!({
                    "uuid": "0199e2d2-aaaa-bbbb-cccc-111122223333",
                    "name": "Huerto Urbano",
                    "description": "Guía de cultivo",
                    "created_at": "2026-03-01T09:00:00Z",
                    "updated_at": "2026-03-01T09:30:00Z",
                    "docs": [{
                        "uuid": "doc-1",
                        "filename": "calendario.md",
                        "content": "Sembrar en marzo.",
                        "created_at": "2026-03-01T09:10:00Z"
                    }]
                }),
            )],
        );

        escribir_zip(
            &carpeta.join("conversations-000.zip"),
            &[(
                "conversations.json",
                json!([{
                    "uuid": "conv-proy",
                    "name": "Consulta del huerto",
                    "created_at": "2026-03-01T10:00:00Z",
                    "updated_at": "2026-03-01T10:05:00Z",
                    "chat_messages": [{
                        "uuid": "m-1",
                        "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
                        "sender": "human",
                        "created_at": "2026-03-01T10:00:00Z",
                        "content": [
                            {
                                "type": "injected_prompt_block",
                                "prompt": "Archivos en /projects/0199e2d2-aaaa-bbbb-cccc-111122223333/files"
                            },
                            {
                                "type": "text",
                                "text": "¿Qué dice el calendario?"
                            }
                        ]
                    }]
                }]),
            )],
        );

        importar(&mut con, &carpeta).unwrap();
        let id_conv: i64 = con
            .query_row(
                "SELECT id FROM conversaciones WHERE id_original = 'conv-proy'",
                [],
                |f| f.get(0),
            )
            .unwrap();
        let det = detalles_web(&con, id_conv).unwrap();
        assert_eq!(det.proyecto.as_deref(), Some("Huerto Urbano"));
        assert!(!det.documentos_proyecto.is_empty());
        assert!(det
            .documentos_proyecto
            .iter()
            .any(|d| d.titulo == "calendario.md"));
    }

    #[test]
    fn conversacion_sin_titulo() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-sin-titulo",
                "name": "",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:01:00Z",
                "chat_messages": [{
                    "uuid": "m-1",
                    "sender": "human",
                    "created_at": "2026-03-01T10:00:00Z",
                    "content": [{ "type": "text", "text": "Receta de pan de masa madre casero" }]
                }]
            }]),
        );

        importar(&mut con, &zip).unwrap();
        let titulo: Option<String> = con
            .query_row("SELECT titulo FROM conversaciones WHERE id = 1", [], |f| {
                f.get(0)
            })
            .unwrap();
        assert_eq!(
            titulo.as_deref(),
            Some("sin proyecto · Receta de pan de masa madre casero")
        );
    }

    #[test]
    fn conversacion_vacia_no_rompe() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-vacia",
                "name": "",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:00:00Z",
                "chat_messages": []
            }]),
        );

        let r = importar(&mut con, &zip).unwrap();
        assert_eq!(r.nuevas, 1);
        let lec = leer(&con, 1, &Tramo::default()).unwrap();
        assert!(lec.bloques.is_empty());
    }

    #[test]
    fn texto_de_adjunto_se_conserva() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-adj",
                "name": "Análisis de informe",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:01:00Z",
                "chat_messages": [{
                    "uuid": "m-1",
                    "sender": "human",
                    "created_at": "2026-03-01T10:00:00Z",
                    "content": [{ "type": "text", "text": "Revisa este documento" }],
                    "attachments": [{
                        "file_name": "informe.pdf",
                        "file_type": "application/pdf",
                        "file_size": 4096,
                        "extracted_content": " TurbinaHidraulicaEspecial "
                    }]
                }]
            }]),
        );

        importar(&mut con, &zip).unwrap();
        let res = buscar(
            &con,
            "TurbinaHidraulicaEspecial",
            &Filtros::default(),
            "cli",
        )
        .unwrap();
        assert_eq!(res.resultados.len(), 1);
        assert_eq!(res.resultados[0].id_publico, "cw-conv-adj");
    }

    #[test]
    fn partes_tecnicas_se_ocultan() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-tec",
                "name": "Bloques internos",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:01:00Z",
                "chat_messages": [{
                    "uuid": "m-1",
                    "sender": "assistant",
                    "created_at": "2026-03-01T10:00:00Z",
                    "content": [
                        { "type": "injected_prompt_block", "prompt": "SecretoInternoPrompt" },
                        { "type": "token_budget", "budget": 1024 },
                        { "type": "flag", "flag": "internal_flag" },
                        { "type": "text", "text": "Respuesta limpia visible" }
                    ]
                }]
            }]),
        );

        importar(&mut con, &zip).unwrap();
        let lec = leer(
            &con,
            1,
            &Tramo {
                todo: true,
                ..Tramo::default()
            },
        )
        .unwrap();
        assert_eq!(lec.bloques.len(), 1);
        let res = buscar(&con, "SecretoInternoPrompt", &Filtros::default(), "cli").unwrap();
        assert!(res.resultados.is_empty());
    }

    #[test]
    fn campo_desconocido_avisa() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-aviso",
                "name": "Con campo nuevo",
                "campo_futuro_inventado": 123,
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:01:00Z",
                "chat_messages": [{
                    "uuid": "m-1",
                    "sender": "human",
                    "created_at": "2026-03-01T10:00:00Z",
                    "content": [{ "type": "tipo_futuro", "text": "hola" }]
                }]
            }]),
        );

        let r = importar(&mut con, &zip).unwrap();
        assert_eq!(r.nuevas, 1);
        assert!(!r.avisos.is_empty());
    }

    #[test]
    fn no_se_confunde_con_chatgpt() {
        let dir = tempdir().unwrap();
        let zip_html = dir.path().join("chatgpt-html.zip");
        escribir_zip(
            &zip_html,
            &[
                ("conversations.json", json!([])),
                ("chat.html", json!("<html></html>")),
            ],
        );
        assert!(reconocer(&zip_html).is_err());

        let zip_mapping = dir.path().join("chatgpt-mapping.zip");
        escribir_zip(
            &zip_mapping,
            &[(
                "conversations.json",
                json!([{
                    "id": "gpt-1",
                    "title": "Chat de OpenAI",
                    "current_node": "n1",
                    "mapping": { "n1": {} }
                }]),
            )],
        );
        assert!(reconocer(&zip_mapping).is_err());
    }

    #[test]
    fn carpeta_importa_todo_menos_light_metadata() {
        let (dir, mut con) = base();
        let carpeta = dir.path().join("claude_export");
        std::fs::create_dir_all(&carpeta).unwrap();

        escribir_zip(
            &carpeta.join("conversations-000.zip"),
            &[(
                "conversations.json",
                json!([{
                    "uuid": "c-1",
                    "name": "Charla uno",
                    "created_at": "2026-03-01T10:00:00Z",
                    "updated_at": "2026-03-01T10:01:00Z",
                    "chat_messages": []
                }]),
            )],
        );
        escribir_zip(
            &carpeta.join("memories-000.zip"),
            &[(
                "memories/acc-1.json",
                json!({
                    "account_uuid": "acc-1",
                    "conversations_memory": "Recuerdo importante",
                    "memory_files": []
                }),
            )],
        );
        std::fs::write(
            carpeta.join("light_metadata-000.zip"),
            b"CorreoPrivadoNoLeer@example.com",
        )
        .unwrap();

        assert_eq!(reconocer(&carpeta).unwrap(), "claude-web");
        let r = importar(&mut con, &carpeta).unwrap();
        assert_eq!(r.nuevas, 2);
        assert!(r.avisos.iter().any(|a| a.contains("light_metadata")));
    }

    #[test]
    fn light_metadata_nunca_se_lee() {
        let (dir, mut con) = base();
        let ruta_light = dir.path().join("light_metadata-000.zip");
        // No es siquiera un ZIP válido: si intentara abrirlo como ZIP diría "dañado", no "desconocido".
        std::fs::write(&ruta_light, b"datos sensibles de cuenta").unwrap();

        let err_rec = reconocer(&ruta_light).unwrap_err();
        assert!(err_rec.contains("desconocido"), "{err_rec}");
        let err_imp = importar(&mut con, &ruta_light).unwrap_err();
        assert!(err_imp.contains("desconocido"), "{err_imp}");
    }

    #[test]
    fn memorias_y_frames_son_documentos() {
        let (dir, mut con) = base();
        let carpeta = dir.path().join("docs_export");
        std::fs::create_dir_all(&carpeta).unwrap();

        escribir_zip(
            &carpeta.join("memories-000.zip"),
            &[(
                "memories/acc-1.json",
                json!({
                    "account_uuid": "acc-1",
                    "conversations_memory": "Preferencias de estudio",
                    "memory_files": [{
                        "path": "notas.md",
                        "content": "Apuntes de Rust",
                        "updated_at": "2026-03-02T10:00:00Z"
                    }]
                }),
            )],
        );

        escribir_zip(
            &carpeta.join("frames-000.zip"),
            &[
                (
                    "artifacts/art-1/artifact.json",
                    json!({
                        "id": "art-1",
                        "active_version": "v1",
                        "updated_at": "2026-03-03T10:00:00Z",
                        "versions": [{
                            "id": "v1",
                            "title": "Panel interactivo",
                            "created_at": "2026-03-03T09:00:00Z"
                        }]
                    }),
                ),
                (
                    "artifacts/art-1/versions/v1.html",
                    json!("<!doctype html><html><body><h1>Panel</h1></body></html>"),
                ),
            ],
        );

        let r = importar(&mut con, &carpeta).unwrap();
        assert_eq!(r.nuevas, 3);

        let tipos: Vec<String> = con
            .prepare("SELECT tipo FROM conversaciones ORDER BY id")
            .unwrap()
            .query_map([], |f| f.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(tipos.iter().all(|t| t == "documento"));

        let id_frame: i64 = con
            .query_row(
                "SELECT id FROM conversaciones WHERE id_original = 'frame-art-1'",
                [],
                |f| f.get(0),
            )
            .unwrap();
        let det = detalles_web(&con, id_frame).unwrap();
        assert_eq!(
            det.html.as_deref(),
            Some("<!doctype html><html><body><h1>Panel</h1></body></html>")
        );
    }

    #[test]
    fn chat_de_diseno_tiene_tipo_diseno() {
        let (dir, mut con) = base();
        let zip = dir.path().join("design_chats-000.zip");
        escribir_zip(
            &zip,
            &[(
                "design_chats/dc-1.json",
                json!({
                    "uuid": "dc-1",
                    "title": "Boceto de tarjeta",
                    "created_at": "2026-03-04T10:00:00Z",
                    "updated_at": "2026-03-04T10:05:00Z",
                    "messages": [{
                        "uuid": "dm-1",
                        "role": "user",
                        "created_at": "2026-03-04T10:00:00Z",
                        "content": {
                            "content": "Diseña una tarjeta sobria",
                            "contentBlocks": [],
                            "attachments": []
                        }
                    }]
                }),
            )],
        );

        importar(&mut con, &zip).unwrap();
        let det = detalles_web(&con, 1).unwrap();
        assert_eq!(det.tipo, "diseno");
    }

    #[test]
    fn original_json_se_conserva() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-orig",
                "name": "Con original",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:01:00Z",
                "chat_messages": []
            }]),
        );

        importar(&mut con, &zip).unwrap();
        let guardado: String = con
            .query_row(
                "SELECT json FROM originales WHERE conversacion_id = 1",
                [],
                |f| f.get(0),
            )
            .unwrap();
        assert!(guardado.contains("\"conv-orig\""));
    }

    #[test]
    fn citas_y_adjuntos_llegan_al_panel() {
        let (dir, mut con) = base();
        let zip = zip_conversaciones(
            dir.path(),
            json!([{
                "uuid": "conv-panel",
                "name": "Fuentes y archivos",
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:02:00Z",
                "chat_messages": [
                    {
                        "uuid": "m-1",
                        "sender": "human",
                        "created_at": "2026-03-01T10:00:00Z",
                        "content": [{ "type": "text", "text": "Mira este archivo" }],
                        "attachments": [{
                            "file_name": "datos.csv",
                            "file_type": "text/csv",
                            "file_size": 512,
                            "extracted_content": "col1,col2\n1,2"
                        }]
                    },
                    {
                        "uuid": "m-2",
                        "parent_message_uuid": "m-1",
                        "sender": "assistant",
                        "created_at": "2026-03-01T10:01:00Z",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "tu-1",
                                "name": "web_search",
                                "input": { "query": "nisaba" }
                            },
                            {
                                "type": "tool_result",
                                "tool_use_id": "tu-1",
                                "name": "web_search",
                                "content": [{ "type": "text", "text": "Diosa sumeria de la escritura" }]
                            },
                            {
                                "type": "text",
                                "text": "Nisaba era la diosa de la escritura.",
                                "citations": [{
                                    "uuid": "cit-1",
                                    "start_index": 0,
                                    "end_index": 6,
                                    "details": {
                                        "type": "web_search_citation",
                                        "url": "https://example.org/nisaba"
                                    }
                                }]
                            }
                        ]
                    }
                ]
            }]),
        );

        importar(&mut con, &zip).unwrap();
        let det = detalles_web(&con, 1).unwrap();
        assert_eq!(det.adjuntos.len(), 1);
        assert_eq!(det.adjuntos[0].nombre, "datos.csv");
        assert_eq!(
            det.adjuntos[0].texto_extraido.as_deref(),
            Some("col1,col2\n1,2")
        );
        assert!(det.adjuntos[0].sin_binario);

        assert_eq!(det.citas.len(), 1);
        assert_eq!(det.citas[0].numero, 1);
        assert_eq!(det.citas[0].url, "https://example.org/nisaba");

        assert_eq!(det.herramientas.len(), 1);
        assert_eq!(det.herramientas[0].nombre, "web_search");
        assert!(det.herramientas[0]
            .resultado
            .as_deref()
            .unwrap_or("")
            .contains("Diosa sumeria"));
    }
}
