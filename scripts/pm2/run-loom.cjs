#!/usr/bin/env node
const { spawn } = require('node:child_process');
const path = require('node:path');

const repoRoot = 'C:\\Users\\heycj\\dev\\worktrees\\loom\\cli-server-backend';
const cargoBin = path.join(process.env.USERPROFILE || '', '.cargo', 'bin');
const cargo = path.join(cargoBin, 'cargo.exe');

const env = {
  ...process.env,
  PATH: `${cargoBin};${process.env.PATH || ''}`,
};

const args = ['run', '-p', 'loom-server', '--', 'serve', '--host', '127.0.0.1', '--port', '18081'];

console.log(`[run-loom] cwd=${repoRoot}`);
console.log(`[run-loom] cargo=${cargo}`);

const child = spawn(cargo, args, {
  cwd: repoRoot,
  stdio: 'inherit',
  env,
  windowsHide: true,
});

const forward = (sig) => { try { child.kill(sig); } catch {} };
process.on('SIGINT', () => forward('SIGINT'));
process.on('SIGTERM', () => forward('SIGTERM'));
process.on('SIGHUP', () => forward('SIGHUP'));

child.on('exit', (code, signal) => {
  console.log(`[run-loom] child exited code=${code} signal=${signal}`);
  process.exit(code ?? 1);
});