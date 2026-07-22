#!/usr/bin/env node
const { spawn } = require('node:child_process');
const path = require('node:path');

const repoRoot = 'C:\\Users\\heycj\\dev\\openchamber-feat-dev';
const webRoot = path.join(repoRoot, 'packages', 'web');
const bunBin = path.join(process.env.USERPROFILE || '', '.bun', 'bin');
const bun = path.join(bunBin, 'bun.exe');

const env = {
  ...process.env,
  PATH: `${bunBin};${process.env.PATH || ''}`,
  OPENCHAMBER_PORT: '3902',
  OPENCHAMBER_HMR_UI_PORT: '5180',
  OPENCHAMBER_HMR_HOST: '127.0.0.1',
  OPENCHAMBER_DISABLE_PWA_DEV: '1',
};

const args = ['x', 'vite', '--force', '--host', '127.0.0.1', '--port', '5180', '--strictPort'];

console.log(`[run-openchamber-web] cwd=${webRoot}`);
console.log(`[run-openchamber-web] vite args=${JSON.stringify(args)}`);

const child = spawn(bun, args, {
  cwd: webRoot,
  stdio: 'inherit',
  env,
  windowsHide: true,
});

const forward = (sig) => { try { child.kill(sig); } catch {} };
process.on('SIGINT', () => forward('SIGINT'));
process.on('SIGTERM', () => forward('SIGTERM'));
process.on('SIGHUP', () => forward('SIGHUP'));

child.on('exit', (code, signal) => {
  console.log(`[run-openchamber-web] child exited code=${code} signal=${signal}`);
  process.exit(code ?? 1);
});