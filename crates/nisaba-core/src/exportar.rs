//! Exportar una conversación a Markdown, avisar de posibles secretos y preparar
//! el comando que reanuda la sesión en su herramienta de origen.

use crate::modelo::Fuente;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hallazgo {
    pub tipo: &'static str,
    /// Enmascarada: 6 primeros caracteres + `…` + 4 últimos.
    pub muestra: String,
}

fn es_clave(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

fn mayuscula_o_cifra(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

fn alfanumerico_o_guion(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

struct Regla {
    tipo: &'static str,
    prefijos: &'static [&'static str],
    resto: fn(char) -> bool,
    minimo: usize,
    /// Longitud exacta del resto (AWS).
    exacto: bool,
}

const REGLAS: &[Regla] = &[
    Regla {
        tipo: "Anthropic",
        prefijos: &["sk-ant-"],
        resto: es_clave,
        minimo: 20,
        exacto: false,
    },
    Regla {
        tipo: "OpenAI",
        prefijos: &["sk-"],
        resto: es_clave,
        minimo: 20,
        exacto: false,
    },
    Regla {
        tipo: "GitHub",
        prefijos: &["ghp_", "github_pat_"],
        resto: es_clave,
        minimo: 20,
        exacto: false,
    },
    Regla {
        tipo: "AWS",
        prefijos: &["AKIA"],
        resto: mayuscula_o_cifra,
        minimo: 16,
        exacto: true,
    },
    Regla {
        tipo: "Slack",
        prefijos: &["xoxb-", "xoxp-"],
        resto: alfanumerico_o_guion,
        minimo: 10,
        exacto: false,
    },
];

fn enmascarar(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let inicio: String = c.iter().take(6).collect();
    let fin: String = c[c.len().saturating_sub(4)..].iter().collect();
    format!("{inicio}…{fin}")
}

/// Claves y tokens con formato conocido, claves privadas y asignaciones
/// `password|token|secret = <valor largo>`. Escaneo a mano: sin dependencia de `regex`.
pub fn posibles_secretos(texto: &str) -> Vec<Hallazgo> {
    let mut hallazgos = Vec::new();
    for regla in REGLAS {
        for prefijo in regla.prefijos {
            for (i, _) in texto.match_indices(prefijo) {
                // Inicio de palabra: `task-ant` no es `sk-ant`.
                if texto[..i].chars().next_back().is_some_and(es_clave) {
                    continue;
                }
                let tras = &texto[i + prefijo.len()..];
                if regla.tipo == "OpenAI" && tras.starts_with("ant-") {
                    continue;
                }
                let n = tras.chars().take_while(|c| (regla.resto)(*c)).count();
                let vale = if regla.exacto {
                    n == regla.minimo
                        && !tras[n..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphanumeric())
                } else {
                    n >= regla.minimo
                };
                if vale {
                    hallazgos.push(Hallazgo {
                        tipo: regla.tipo,
                        muestra: enmascarar(&texto[i..i + prefijo.len() + n]),
                    });
                }
            }
        }
    }
    for linea in texto.lines() {
        if let Some(i) = linea.find("-----BEGIN ") {
            if linea[i..].contains("PRIVATE KEY-----") {
                hallazgos.push(Hallazgo {
                    tipo: "Clave privada",
                    muestra: enmascarar(linea[i..].trim_end()),
                });
            }
        }
    }
    // Las mayúsculas ASCII no cambian de longitud: los índices valen para `texto`.
    let minus = texto.to_ascii_lowercase();
    for clave in ["password", "token", "secret"] {
        for (i, _) in minus.match_indices(clave) {
            let tras = texto[i + clave.len()..].trim_start_matches(['"', '\'', ' ']);
            let Some(valor) = tras.strip_prefix(['=', ':']) else {
                continue;
            };
            let valor = valor.trim_start_matches([' ', '"', '\'']);
            // `${VAR}`, `<tu-clave>` o `{{x}}` son marcadores, no secretos.
            if valor.starts_with(['$', '<', '{']) {
                continue;
            }
            let fin = valor
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                .unwrap_or(valor.len());
            if valor[..fin].chars().count() >= 12 {
                hallazgos.push(Hallazgo {
                    tipo: "Contraseña/token",
                    muestra: enmascarar(&valor[..fin]),
                });
            }
        }
    }
    hallazgos
}

/// `AAAA-MM-DD_<proyecto>_<titulo>.ext`, en minúsculas, con guiones y sin
/// caracteres prohibidos en Windows; como mucho 120 caracteres antes de la extensión.
pub fn nombre_archivo(
    fin: Option<&str>,
    proyecto: Option<&str>,
    titulo: &str,
    ext: &str,
) -> String {
    let limpiar = |s: &str| -> String {
        let mut out = String::new();
        for c in s.to_lowercase().chars() {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                continue;
            }
            let c = if c.is_whitespace() { '-' } else { c };
            if !(c == '-' && out.ends_with('-')) {
                out.push(c);
            }
        }
        out.trim_matches(['-', '.']).to_string()
    };
    let fecha = fin.filter(|f| f.len() >= 10).map(|f| f[..10].to_string());
    let partes: Vec<String> = [fecha, proyecto.map(limpiar), Some(limpiar(titulo))]
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect();
    let base = if partes.is_empty() {
        "conversacion".to_string()
    } else {
        partes.join("_")
    };
    let base: String = base.chars().take(120).collect();
    format!("{}.{ext}", base.trim_end_matches(['-', '.', '_']))
}

/// La conversación completa en Markdown, con una cabecera de datos.
/// `todo` incluye herramientas, razonamiento y eventos ocultos, como "Ver todo".
pub fn documento(conn: &Connection, id_publico: &str, todo: bool) -> Result<String, String> {
    use crate::busqueda::{resolver, Resolucion, EXCLUIDA};
    let e = |e: rusqlite::Error| e.to_string();
    let cid = match resolver(conn, id_publico).map_err(e)? {
        Resolucion::Uno(c) => c,
        Resolucion::Ninguno => return Err(format!("No encuentro la conversación {id_publico}")),
        Resolucion::Ambiguo(ids) => {
            return Err(format!(
                "Id ambiguo: {id_publico} encaja con {}",
                ids.join(", ")
            ))
        }
        Resolucion::Excluida => return Err(EXCLUIDA.to_string()),
    };
    let tramo = crate::lector::Tramo {
        cantidad: usize::MAX,
        todo,
        ..Default::default()
    };
    let l = crate::lector::leer(conn, cid, &tramo).map_err(e)?;
    let (proyecto, inicio, fin): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT proyecto, inicio, fin FROM conversaciones WHERE id = ?1",
            [cid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(e)?;
    let guion = || "—".to_string();
    let mut s = format!("# {}\n\n", l.titulo);
    s.push_str(&format!("- **Fuente:** {}\n", l.fuente.as_str()));
    s.push_str(&format!(
        "- **Proyecto:** {}\n",
        proyecto.unwrap_or_else(guion)
    ));
    s.push_str(&format!(
        "- **Fechas:** {} – {}\n",
        inicio.unwrap_or_else(guion),
        fin.unwrap_or_else(guion)
    ));
    s.push_str(&format!(
        "- **Id:** `{}` · `{}`\n",
        l.id_publico, l.id_original
    ));
    if let Some(cwd) = &l.cwd {
        s.push_str(&format!("- **Carpeta:** `{cwd}`\n"));
    }
    s.push_str("\n---\n\n");
    s.push_str(&crate::lector::bloques_a_markdown(&l.bloques));
    Ok(s)
}

/// Comando para reanudar la sesión. Claude Code agrupa las sesiones por carpeta,
/// así que hay que entrar antes en la original. Quien llama decide no ofrecerlo
/// en subagentes (`Lectura::es_subagente`).
pub fn comando_continuar(
    fuente: Fuente,
    id_original: &str,
    cwd: Option<&str>,
    windows: bool,
) -> Option<String> {
    if id_original.is_empty() {
        return None;
    }
    Some(match (fuente, cwd) {
        (Fuente::Codex, _) => format!("codex resume {id_original}"),
        (Fuente::ClaudeCode, None) => format!("claude --resume {id_original}"),
        (Fuente::ClaudeCode, Some(c)) if windows => {
            format!("Set-Location \"{c}\"; claude --resume {id_original}")
        }
        (Fuente::ClaudeCode, Some(c)) => format!("cd \"{c}\" && claude --resume {id_original}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tipos(t: &str) -> Vec<&'static str> {
        posibles_secretos(t).into_iter().map(|h| h.tipo).collect()
    }

    #[test]
    fn detecta_cada_tipo_de_secreto() {
        assert_eq!(
            tipos("clave sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv fin"),
            ["Anthropic"]
        );
        assert_eq!(tipos("OPENAI=sk-proj-AbCdEfGhIjKlMnOpQrStUvWx"), ["OpenAI"]);
        assert_eq!(
            tipos("ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
            ["GitHub"]
        );
        assert_eq!(
            tipos("github_pat_11ABCDEFG0123456789_abcdefghij"),
            ["GitHub"]
        );
        assert_eq!(tipos("id AKIAIOSFODNN7EXAMPLE."), ["AWS"]);
        assert_eq!(tipos("xoxb-123456789012-abcdefghijkl"), ["Slack"]);
        assert_eq!(
            tipos("-----BEGIN OPENSSH PRIVATE KEY-----\nb3Blbn\n-----END OPENSSH PRIVATE KEY-----"),
            ["Clave privada"]
        );
        assert_eq!(
            tipos("DB_PASSWORD = \"s3cr3t0-muy-largo\""),
            ["Contraseña/token"]
        );
        assert_eq!(tipos("token: abcdefghijklmnop"), ["Contraseña/token"]);
        assert_eq!(
            tipos("a sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv y ghp_AbCdEfGhIjKlMnOpQrStUvWxYz01").len(),
            2
        );
    }

    #[test]
    fn no_marca_texto_normal() {
        for t in [
            "El token de acceso caduca en una hora; cambia el password del usuario.",
            "secret: corto",
            "task-ant-farm y desk-chair",
            "La clave pública (PUBLIC KEY) no es secreta",
            "AKIA solo",
            "token = ${API_TOKEN}",
        ] {
            assert!(posibles_secretos(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn muestra_enmascarada() {
        let h = &posibles_secretos("sk-ant-api03-AbCdEfGhIjKlMnOpQra3F9")[0];
        assert_eq!(h.muestra, "sk-ant…a3F9");
        assert!(!h.muestra.contains("AbCdEf"));
    }

    #[test]
    fn nombre_archivo_sanea_caracteres() {
        assert_eq!(
            nombre_archivo(
                Some("2026-10-29T10:00:00.000Z"),
                Some("Mirlo App"),
                "¿Qué es: a/b <c>|d*e?\"f\\g",
                "md"
            ),
            "2026-10-29_mirlo-app_¿qué-es-ab-cdefg.md"
        );
        let largo = nombre_archivo(None, None, &"palabra ".repeat(40), "pdf");
        assert!(largo.ends_with(".pdf"));
        assert!(largo.chars().count() <= 124);
        assert!(!largo.trim_end_matches(".pdf").ends_with('-'));
    }

    #[test]
    fn nombre_archivo_sin_fecha_ni_proyecto() {
        assert_eq!(
            nombre_archivo(None, None, "Hola mundo", "md"),
            "hola-mundo.md"
        );
        assert_eq!(nombre_archivo(None, Some(""), "", "md"), "conversacion.md");
    }

    #[test]
    fn comando_continuar_por_fuente_y_sistema() {
        let id = "a1b2c3d4-0000-4000-8000-000000000001";
        assert_eq!(
            comando_continuar(Fuente::ClaudeCode, id, Some("/home/u/P"), false).unwrap(),
            format!("cd \"/home/u/P\" && claude --resume {id}")
        );
        assert_eq!(
            comando_continuar(Fuente::ClaudeCode, id, Some("C:\\P"), true).unwrap(),
            format!("Set-Location \"C:\\P\"; claude --resume {id}")
        );
        assert_eq!(
            comando_continuar(Fuente::ClaudeCode, id, None, false).unwrap(),
            format!("claude --resume {id}")
        );
        assert_eq!(
            comando_continuar(Fuente::Codex, "019a-cx", Some("/x"), true).unwrap(),
            "codex resume 019a-cx"
        );
        assert_eq!(comando_continuar(Fuente::Codex, "", None, false), None);
    }

    #[test]
    fn documento_incluye_cabecera_y_conversacion_completa() {
        use crate::indexador::{indexar, Rutas};
        let dir = tempfile::tempdir().unwrap();
        let rutas = Rutas {
            home: dir.path().join("home"),
            claude: dir.path().join("claude"),
            codex: dir.path().join("codex"),
        };
        let sid = "d0c0e000-0000-4000-8000-000000000009";
        let carpeta = rutas.claude.join("-home-u-Larga");
        std::fs::create_dir_all(&carpeta).unwrap();
        let mut s = String::new();
        let mut padre = "null".to_string();
        for i in 0..30 {
            let (tipo, rol) = if i % 2 == 0 {
                ("user", "user")
            } else {
                ("assistant", "assistant")
            };
            let contenido = if rol == "user" {
                format!("\"pregunta número {i}\"")
            } else {
                format!("[{{\"type\":\"text\",\"text\":\"respuesta número {i}\"}}]")
            };
            s.push_str(&format!(
                "{{\"type\":\"{tipo}\",\"uuid\":\"m{i}\",\"parentUuid\":{padre},\"sessionId\":\"{sid}\",\"cwd\":\"/home/u/Larga\",\"timestamp\":\"2026-10-0{}T10:00:{i:02}.000Z\",\"message\":{{\"role\":\"{rol}\",\"content\":{contenido}}}}}\n",
                if i < 15 { 1 } else { 2 }
            ));
            padre = format!("\"m{i}\"");
        }
        std::fs::write(carpeta.join(format!("{sid}.jsonl")), s).unwrap();
        let mut conn = crate::db::abrir(&rutas.home.join("tablilla.db")).unwrap();
        indexar(&mut conn, &rutas, &mut |_| {});

        let doc = documento(&conn, "cc-d0c0e000", false).unwrap();
        assert!(doc.starts_with("# "), "{doc}");
        for esperado in [
            "claude-code",
            "Larga",
            "/home/u/Larga",
            "cc-d0c0e000",
            sid,
            "2026-10-01",
            "2026-10-02",
            "pregunta número 0",
            "respuesta número 29",
        ] {
            assert!(doc.contains(esperado), "falta {esperado}:\n{doc}");
        }
        assert!(documento(&conn, "cc-ffffffff", false)
            .unwrap_err()
            .contains("No encuentro"));
    }
}
