//! Formato de texto compartido entre la CLI y el MCP. Nunca imprime: devuelve `String`.

use nisaba_core::busqueda::Respuesta;

/// Pinta «…» en negrita ANSI si hay terminal.
pub fn resaltar(s: &str, tty: bool) -> String {
    if !tty {
        return s.to_string();
    }
    s.replace('«', "\x1b[1m«").replace('»', "»\x1b[0m")
}

/// Texto de los resultados de búsqueda. `con_orden` añade el `alrededor_de`
/// que `leer_conversacion` necesita para centrar el tramo (solo MCP).
pub fn texto_busqueda(r: &Respuesta, tty: bool, con_orden: bool) -> String {
    let mut s = String::new();
    for x in &r.resultados {
        let fecha: String = x.fin.as_deref().unwrap_or("").chars().take(10).collect();
        s.push_str(&format!("{}  {}\n", x.id_publico, x.titulo));
        s.push_str(&format!("   {} · {} · {} coincidencias\n", x.fuente.as_str(), fecha, x.coincidencias));
        s.push_str(&format!("   {}\n", resaltar(&x.fragmento, tty)));
        if con_orden {
            s.push_str(&format!("   alrededor_de: {}\n", x.orden_mensaje));
        }
    }
    if let Some(sug) = &r.sugerencia {
        s.push_str(sug);
        s.push('\n');
    }
    s
}
