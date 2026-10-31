use std::fs;
use std::io::Write;
use std::path::Path;

use nisaba_core::archivo_web;
use nisaba_core::boveda;
use nisaba_core::busqueda::{self, Filtros};
use nisaba_core::importar;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn escribir_zip(ruta: &Path, entradas: &[(&str, &[u8])]) {
    let f = fs::File::create(ruta).unwrap();
    let mut zw = ZipWriter::new(f);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for &(nombre, datos) in entradas {
        zw.start_file(nombre, opts).unwrap();
        zw.write_all(datos).unwrap();
    }
    zw.finish().unwrap();
}

fn contiene_en_claro(dir: &Path, marcador: &[u8]) -> bool {
    let Ok(entradas) = fs::read_dir(dir) else {
        return false;
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        if ruta.is_dir() {
            if contiene_en_claro(&ruta, marcador) {
                return true;
            }
        } else if let Ok(bytes) = fs::read(&ruta) {
            if bytes.windows(marcador.len()).any(|v| v == marcador) {
                return true;
            }
        }
    }
    false
}

#[test]
fn marcador_de_fixture_no_queda_en_claro() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let origen = dir.path().join("origen");
    let tmp = dir.path().join("tmp");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&origen).unwrap();
    fs::create_dir_all(&tmp).unwrap();

    let marcador = "MARCADOR_SECRETO_ACEPTACION_V012_CLAUDE_WEB";
    let conv_json = format!(
        r#"[{{
            "uuid": "a1b2c3d4-1111-4000-8000-000000000099",
            "name": "Charla con {marcador}",
            "created_at": "2026-10-31T10:00:00Z",
            "updated_at": "2026-10-31T10:05:00Z",
            "chat_messages": [{{
                "uuid": "m-001",
                "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
                "sender": "human",
                "text": "Texto con {marcador}",
                "created_at": "2026-10-31T10:00:00Z",
                "content": [{{"type": "text", "text": "Texto con {marcador}"}}],
                "attachments": [{{"file_name": "nota.txt", "file_type": "text/plain", "file_size": 42, "extracted_content": "Adjunto {marcador}"}}]
            }}]
        }}]"#
    );
    escribir_zip(
        &origen.join("conversations-001.zip"),
        &[("conversations.json", conv_json.as_bytes())],
    );

    let nueva = boveda::preparar("contrasena segura de prueba 123").unwrap();
    let llave = nueva.guardar(&home).unwrap();
    {
        let mut conn = archivo_web::abrir(&home, &llave).unwrap();
        let res = importar::importar(&mut conn, &origen).unwrap();
        assert_eq!(res.nuevas, 1);
    }

    assert!(
        !contiene_en_claro(&home, marcador.as_bytes()),
        "El marcador apareció sin cifrar dentro de NISABA_HOME"
    );
    assert!(
        !contiene_en_claro(&tmp, marcador.as_bytes()),
        "El marcador apareció sin cifrar dentro de la carpeta temporal"
    );

    let conn = archivo_web::abrir(&home, &llave).unwrap();
    let r = busqueda::buscar_sin_contar(
        &conn,
        marcador,
        &Filtros {
            limite: 10,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.resultados.len(), 1);
    assert_eq!(r.resultados[0].id_publico, "cw-a1b2c3d4");
}
