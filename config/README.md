# config

Load configuration from XDG `config.toml` and optional project `.env`, then apply it to the process environment. Single place for all env/config used by Loom and related tools.

## Priority

Variables are applied only when **not already set** in the process. Precedence (highest first):

1. **Existing environment** — already set in the process
2. **Project `.env`** — from current directory or `override_dir`
3. **XDG config** — `$XDG_CONFIG_HOME/<app_name>/config.toml` `[env]` table

## Usage

```rust
use config::load_and_apply;

// Load from ~/.config/loom/config.toml and optional .env in current dir
load_and_apply("loom", None)?;

// Load .env from a specific directory instead of current dir
load_and_apply("loom", Some(project_root.as_path()))?;
```

After calling `load_and_apply`, use `std::env::var("KEY")` as usual; no API changes in the rest of the app.

## XDG config file

Location: `$XDG_CONFIG_HOME/<app_name>/config.toml` (e.g. `~/.config/loom/config.toml` on Linux/macOS).

Example:

```toml
[env]
OPENAI_API_KEY = "sk-..."
OPENAI_API_BASE = "https://api.openai.com/v1"
RUST_LOG = "info"
```

Only the `[env]` table is read; keys are injected as environment variables when not already set.

## Project .env

A `.env` file in the project directory (or `override_dir`) is supported. Format: `KEY=VALUE` per line, with `#` comments and optional double/single quotes. Missing file is ignored (empty map).

## Errors

`load_and_apply` returns `Result<(), config::LoadError>` for XDG path, read, or TOML parse failures. Missing config file is not an error (treated as empty).
