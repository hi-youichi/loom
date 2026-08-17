// pm2 process definitions for the OpenChamber + Loom dev environment.
//
// Usage (from the repo root):
//   pm2 start scripts/ecosystem.config.cjs
//   cargo build -p cli; pm2 restart loom-dev     # after Rust changes
//   pm2 logs loom-dev --lines 100
//
// Notes:
// - Requires `cargo build -p cli` first: pm2 runs target/debug/loom.exe
//   directly (no `cargo run` wrapper, so signals reach the server).
// - The 3030 default instance (~/.loom) is NOT managed here; only the
//   isolated 3031 dev instance.
// - LLM creds (OPENAI_API_KEY / OPENAI_BASE_URL) are snapshotted from the
//   shell at `pm2 start`; refresh with `pm2 restart loom-dev --update-env`.
// - `watch` is intentionally disabled: target/ churn would cause endless
//   restarts. Restart manually after rebuilding.

const path = require("path");

const LOOM_ROOT = path.resolve(__dirname, "..");
const CHAMBER_ROOT = path.resolve(__dirname, "..", "..", "openchamber-feat-dev");

const HOME = process.env.USERPROFILE || process.env.HOME;

module.exports = {
  apps: [
    {
      name: "loom-dev",
      script: LOOM_ROOT + "/target/debug/loom.exe",
      args: "server --port 3031 --home .loom-home --pid-file .loom-home/loom-server.pid",
      cwd: LOOM_ROOT,
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
