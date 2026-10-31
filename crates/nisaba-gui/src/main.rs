#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Emitter, Manager};

mod aperturas;
mod archivo;
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(aperturas)
        .manage(archivo::Archivo::default())
        .manage(indexacion::Indexacion::default())
        .on_window_event(|ventana, evento| {
            if matches!(evento, tauri::WindowEvent::Destroyed) {
                archivo::bloquear_en(ventana.state::<archivo::Archivo>().inner());
            }
        })
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
            comandos::documento,
            comandos::guardar_como,
            comandos::origen,
            comandos::abrir_carpeta,
            comandos::exclusiones_lista,
            comandos::contar_en_carpeta,
            comandos::excluir,
            comandos::quitar_exclusion,
            comandos::proyectos_con_carpeta,
            comandos::elegir_carpeta,
            archivo::estado_archivo,
            archivo::preparar_archivo,
            archivo::confirmar_archivo,
            archivo::desbloquear,
            archivo::bloquear,
            archivo::recuperar_archivo,
            archivo::cambiar_contrasena,
            archivo::importar_zip,
            archivo::elegir_zip,
            archivo::elegir_carpeta_importacion,
            archivo::versiones,
            archivo::leer_version,
            archivo::detalles_web,
            archivo::leer_rama,
            indexacion::iniciar,
            indexacion::estado_indexacion,
            aperturas::apertura_inicial
        ])
        .run(tauri::generate_context!())
        .expect("No se pudo abrir Nisaba");
}
