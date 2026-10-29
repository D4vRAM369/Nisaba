use crate::modelo::{Mensaje, Rol, TipoParte};

const GENERICOS: [&str; 13] = [
    "continúa",
    "continua",
    "sigue",
    "ok",
    "vale",
    "si",
    "sí",
    "dale",
    "adelante",
    "continue",
    "go on",
    "go ahead",
    "yes",
];
const MAX_TITULO: usize = 80;

fn es_generico(texto: &str) -> bool {
    let t = texto
        .trim()
        .trim_end_matches(|c: char| c.is_ascii_punctuation() || "¡¿…".contains(c))
        .to_lowercase();
    GENERICOS.contains(&t.as_str())
}

fn candidato(texto: &str) -> bool {
    let t = texto.trim();
    !t.starts_with('/') && !t.starts_with('<') && t.chars().count() >= 15 && !es_generico(t)
}

/// Título legible: "{proyecto} · {primer mensaje de usuario con sustancia}".
pub fn titulo_util(proyecto: Option<&str>, mensajes: &[Mensaje]) -> String {
    let proyecto = proyecto.filter(|p| !p.is_empty()).unwrap_or("sin proyecto");
    let texto = mensajes
        .iter()
        .filter(|m| m.rol == Rol::Usuario && !m.oculto)
        .flat_map(|m| {
            m.partes
                .iter()
                .filter(|p| p.tipo == TipoParte::Texto)
                .take(1)
        })
        .find(|p| candidato(&p.contenido))
        .map(|p| {
            p.contenido
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "(sin título)".to_string());
    let completo = format!("{proyecto} · {texto}");
    if completo.chars().count() <= MAX_TITULO {
        completo
    } else {
        let mut corto: String = completo.chars().take(MAX_TITULO - 1).collect();
        corto.push('…');
        corto
    }
}
