#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

mod comandos;
mod indexacion;

fn main() {
    tauri::Builder::default()
        .manage(indexacion::Indexacion::default())
        .setup(|app| {
            app.state::<indexacion::Indexacion>()
                .iniciar(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            comandos::buscar,
            comandos::leer,
            comandos::salud,
            comandos::proyectos,
            comandos::aviso_limpieza,
            indexacion::iniciar,
            indexacion::estado_indexacion
        ])
        .run(tauri::generate_context!())
        .expect("No se pudo abrir Nisaba");
}
