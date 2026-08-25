// pm2 process definitions for the anureo + anureo dev environment.
//
// Usage (from the repo root):
//   pm2 start scripts/ecosystem.config.cjs
//   cargo build -p anureo-cli; pm2 restart anureo-dev     # after Rust changes
//   pm2 logs anureo-dev --lines 100
//
// Notes:
// - Requires `cargo build -p anureo-cli` first: pm2 runs target/debug/anureo.exe
//   directly (no `cargo run` wrapper, so signals reach the server).
// - The 3030 default instance (~/.anureo) is NOT managed here; only the
//   isolated 3031 dev instance.
// - LLM creds (OPENAI_API_KEY / OPENAI_BASE_URL) are snapshotted from the
//   shell at `pm2 start`; refresh with `pm2 restart anureo-dev --update-env`.
// - `watch` is intentionally disabled: target/ churn would cause endless
//   restarts. Restart manually after rebuilding.

const path = require("path");

const ANUREO_ROOT = path.resolve(__dirname, "..");
const CHAMBER_ROOT = process.env.ANUREO_FRONTEND_ROOT
  ? path.resolve(process.env.ANUREO_FRONTEND_ROOT)
  : path.resolve(__dirname, "..", "..", "openchamber-feat-dev");

const HOME = process.env.USERPROFILE || process.env.HOME;

module.exports = {
  apps: [
    {
      name: "anureo-dev",
      script: ANUREO_ROOT + "/target/debug/anureo.exe",
      args: "server --port 3031 --home .anureo-home --pid-file .anureo-home/anureo-server.pid --log-level trace --log-file .anureo-home/anureo-dev.log",
      cwd: ANUREO_ROOT,
      time: true,
      max_restarts: 10,
      min_uptime: "10s",
    },
    {
      name: "chamber-dev",
      // npm's bun.cmd shim cannot be forked by pm2 on Windows; use the real exe.
      script: HOME + "\\.bun\\bin\\bun.exe",
      args: "run dev",
      cwd: CHAMBER_ROOT,
      time: true,
    },
  ],
};
