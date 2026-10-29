use std::sync::Mutex;

#[derive(Default)]
pub struct Aperturas(Mutex<Option<String>>);

pub fn argumento_apertura(argumentos: &[String]) -> Result<Option<String>, String> {
    let mut args = argumentos.iter().skip(1);
    match args.next() {
        None => Ok(None),
        Some(opcion) if opcion == "--abrir" => {
            let id = args
                .next()
                .filter(|s| !s.is_empty() && !s.starts_with('-'))
                .ok_or("Falta el id: usa nisaba-gui --abrir <id>")?;
            if args.next().is_some() {
                return Err("Argumentos ambiguos: usa nisaba-gui --abrir <id>".into());
            }
            Ok(Some(id.clone()))
        }
        Some(opcion) => Err(format!(
            "Argumento desconocido: {opcion}. Usa nisaba-gui [--abrir <id>]"
        )),
    }
}

impl Aperturas {
    pub fn guardar(&self, id: String) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
    }
    pub fn tomar(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[tauri::command]
pub fn apertura_inicial(aperturas: tauri::State<'_, Aperturas>) -> Option<String> {
    aperturas.tomar()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }
    #[test]
    fn abrir_conserva_el_id_literal() {
        assert_eq!(
            argumento_apertura(&args(&["nisaba-gui", "--abrir", "cc-%_literal"])).unwrap(),
            Some("cc-%_literal".into())
        );
        assert_eq!(argumento_apertura(&args(&["nisaba-gui"])).unwrap(), None);
    }
    #[test]
    fn abrir_rechaza_valor_ausente_y_argumentos_ambiguos() {
        for a in [
            &["nisaba-gui", "--abrir"][..],
            &["nisaba-gui", "--abrir", "--otra"],
            &["nisaba-gui", "--abrir", "cc-id", "--abrir", "cx-otro"],
            &["nisaba-gui", "--desconocido"],
        ] {
            assert!(argumento_apertura(&args(a)).is_err(), "{a:?}");
        }
    }
    #[test]
    fn apertura_pendiente_se_entrega_una_vez_y_conserva_la_mas_reciente() {
        let pendientes = Aperturas::default();
        pendientes.guardar("cc-primera".into());
        pendientes.guardar("cx-segunda".into());
        assert_eq!(pendientes.tomar(), Some("cx-segunda".into()));
        assert_eq!(pendientes.tomar(), None);
    }
}
