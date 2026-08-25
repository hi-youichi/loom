// PM2 config for the LOCAL stack (isolated from dev 3031/3101 and the
// production C:\srv Caddy stack on 3041/3111).
// Ports: anureo=3051, Express=3151. No UI password on either side (both stay in
// dev mode). Data dir is isolated so a later password login can never mint
// ~/.config/anureo/jwt-secret and flip every other instance out of dev mode.
module.exports = {
  apps: [
    {
      name: 'anureo-local',
      script: 'target/debug/anureo.exe',
      args: 'server --port 3051 --home .anureo-home-local --pid-file .anureo-home-local/anureo-server.pid',
      cwd: 'C:\\Users\\heycj\\dev\\anureo',
      env: {
        ANUREO_DATA_DIR: 'C:\\Users\\heycj\\dev\\anureo\\.anureo-home-local\\anureo-data',
      },
      error_file: 'C:\\Users\\heycj\\dev\\anureo\\.anureo-home-local\\logs\\anureo-local-error.log',
      out_file: 'C:\\Users\\heycj\\dev\\anureo\\.anureo-home-local\\logs\\anureo-local-out.log',
      log_date_format: 'YYYY-MM-DD HH:mm:ss',
      merge_logs: true,
      autorestart: true,
      watch: false,
      max_memory_restart: '1G',
    },
    {
      name: 'chamber-local',
      script: 'node',
      args: 'packages/web/server/index.js --port 3151',
      cwd: 'C:\\Users\\heycj\\dev\\anureo-feat-dev',
      env: {
        PORT: '3151',
        NODE_ENV: 'production',
        OPENCODE_SKIP_START: 'true',
        ANUREO_ACP_BASE_URL: 'http://127.0.0.1:3051',
        ANUREO_DATA_DIR: 'C:\\Users\\heycj\\dev\\anureo\\.anureo-home-local\\anureo-data',
      },
      error_file: 'C:\\Users\\heycj\\dev\\anureo\\.anureo-home-local\\logs\\chamber-local-error.log',
      out_file: 'C:\\Users\\heycj\\dev\\anureo\\.anureo-home-local\\logs\\chamber-local-out.log',
      log_date_format: 'YYYY-MM-DD HH:mm:ss',
      merge_logs: true,
      autorestart: true,
      watch: false,
      max_memory_restart: '1G',
    },
  ],
};
