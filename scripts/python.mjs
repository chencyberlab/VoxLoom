// Runs Python for the npm scripts on every platform. Windows has no python3
// (that name opens the Microsoft Store), and its Python reads files in the
// ANSI code page unless UTF-8 mode is on. PYTHON overrides the interpreter.
import { spawnSync } from 'node:child_process';

const windows = process.platform === 'win32';
const python = process.env.PYTHON || (windows ? 'python' : 'python3');
const env = windows ? { ...process.env, PYTHONUTF8: '1' } : process.env;
const result = spawnSync(python, process.argv.slice(2), { stdio: 'inherit', env });
if (result.error) {
  console.error(`Cannot run ${python}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
