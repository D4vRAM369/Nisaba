// Copia la CLI compilada al nombre que Tauri espera para un sidecar: binaries/nisaba-<target-triple>[.exe].
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const gui = join(dirname(fileURLToPath(import.meta.url)), '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const origen = join(gui, '..', '..', 'target', 'release', `nisaba${exe}`);
if (!existsSync(origen)) {
  console.error('Compila antes la CLI: cargo build --release -p nisaba-cli');
  process.exit(1);
}
const triple = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (\S+)$/m)[1];
mkdirSync(join(gui, 'binaries'), { recursive: true });
const destino = join(gui, 'binaries', `nisaba-${triple}${exe}`);
copyFileSync(origen, destino);
console.log(`Sidecar listo: ${destino}`);
