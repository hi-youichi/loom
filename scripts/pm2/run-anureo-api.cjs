#!/usr/bin/env node
const { spawn } = require('node:child_process');
const path = require('node:path');

const repoRoot = 'C:\\Users\\heycj\\dev\\anureo-feat-dev';
const bunBin = path.join(process.env.USERPROFILE || '', '.bun', 'bin');
const bun = path.join(bunBin, 'bun.exe');

const env = {
  ...process.env,
  PATH: `${bunBin};${process.env.PATH || ''}`,
  OPENCODE_HOST: 'http://127.0.0.1:18081',
  OPENCODE_SKIP_START: 'true',
  ANUREO_PORT: '3902',
  ANUREO_HMR_API_PORT: '3902',
};

const args = ['--cwd', 'packages/web', 'server/index.js', '--port', '3902'];

console.log(`[run-anureo-api] cwd=${repoRoot}`);
console.log(`[run-anureo-api] bun=${bun}`);
console.log(`[run-anureo-api] args=${JSON.stringify(args)}`);
console.log(`[run-anureo-api] OPENCODE_HOST=${env.OPENCODE_HOST}`);

const child = spawn(bun, args, {
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
  console.log(`[run-anureo-api] child exited code=${code} signal=${signal}`);
  process.exit(code ?? 1);
});