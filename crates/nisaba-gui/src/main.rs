#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Emitter, Manager};

mod aperturas;
mod comandos;
mod indexacion;

fn main() {
    let inicial = match aperturas::argumento_apertura(&std::env::args().collect::<Vec<_>>()) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let aperturas = aperturas::Aperturas::default();
    if let Some(id) = inicial {
        aperturas.guardar(id);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match aperturas::argumento_apertura(&args) {
                Ok(Some(id)) => {
                    app.state::<aperturas::Aperturas>().guardar(id.clone());
                    if let Err(e) = app.emit("abrir-conversacion", &id) {
                        eprintln!("No se pudo abrir la conversación en la ventana: {e}");
                    }
                }
                Ok(None) => {}
                Err(e) => eprintln!("{e}"),
            }
            if let Some(ventana) = app.get_webview_window("main") {
                for resultado in [ventana.show(), ventana.unminimize(), ventana.set_focus()] {
                    if let Err(e) = resultado {
                        eprintln!("No se pudo traer Nisaba al frente: {e}");
                    }
                }
            }
        }))
        .manage(aperturas)
        .manage(indexacion::Indexacion::default())
        .setup(|app| {
            app.state::<indexacion::Indexacion>()
                .iniciar(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            comandos::buscar,
            comandos::recientes,
            comandos::contar_busqueda,
            comandos::leer,
            comandos::salud,
            comandos::proyectos,
            comandos::aviso_limpieza,
            indexacion::iniciar,
            indexacion::estado_indexacion,
            aperturas::apertura_inicial
        ])
        .run(tauri::generate_context!())
        .expect("No se pudo abrir Nisaba");
}
