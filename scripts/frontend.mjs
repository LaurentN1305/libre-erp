// Hooks Tauri indépendants d'une installation globale de npm/pnpm.
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
const mode = process.argv[2];
function run(script, args) {
  return new Promise((ok, fail) => {
    const child = spawn(process.execPath, [resolve(script), ...args], { stdio: 'inherit', shell: false });
    child.on('error', fail);
    child.on('exit', code => code === 0 ? ok() : fail(new Error(`Compilation frontend interrompue (${code}).`)));
    for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => child.kill(signal));
  });
}
if (mode === 'build') {
  await run('node_modules/typescript/bin/tsc', ['--noEmit']);
  await run('node_modules/vite/bin/vite.js', ['build']);
} else if (mode === 'dev') {
  await run('node_modules/vite/bin/vite.js', ['--host', '127.0.0.1', '--port', '1420', '--strictPort']);
} else { throw new Error('Mode attendu : build ou dev'); }
