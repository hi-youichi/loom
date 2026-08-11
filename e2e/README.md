# Loom Node.js E2E

## ACP BDD tests

The ACP tests use Node's built-in test runner. The test process talks to the
stdio bridge; the bridge owns the WebSocket connection. Test names follow
`Given / When / Then` wording, while the corresponding Gherkin
living specifications are under `features/acp/`.

Build the CLI binary first:

```powershell
cargo build -p cli
```

Run the ACP BDD suite from the repository root:

```powershell
npm --prefix e2e run test:bdd:acp
```

Use a different executable when needed:

```powershell
$env:LOOM_BIN = "C:\path\to\loom.exe"
npm --prefix e2e run test:bdd:acp
```

The current suite executes both the implemented `loom acp` stdio bridge and
`loom --acp` CLI-client session/prompt scenarios. The prompt scenario uses a
deterministic Node ACP WebSocket fixture, so it does not require network access
or an LLM provider.
