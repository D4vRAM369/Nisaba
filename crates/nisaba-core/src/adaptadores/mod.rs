pub mod claude_code;
pub mod codex;

const MAX_HERRAMIENTA: usize = 64 * 1024;
const SUFIJO_RECORTE: &str = "\n…[recortado]";

/// Recorta a 64 KiB (en frontera de carácter) y marca el recorte.
pub(crate) fn recortar(mut s: String) -> String {
    if s.len() <= MAX_HERRAMIENTA {
        return s;
    }
    let mut corte = MAX_HERRAMIENTA;
    while !s.is_char_boundary(corte) {
        corte -= 1;
    }
    s.truncate(corte);
    s.push_str(SUFIJO_RECORTE);
    s
}

/// Añade un aviso solo si no estaba ya (un aviso por tipo).
pub(crate) fn avisar(avisos: &mut Vec<String>, texto: String) {
    if !avisos.contains(&texto) {
        avisos.push(texto);
    }
}
