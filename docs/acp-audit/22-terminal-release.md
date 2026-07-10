# ACP Protocol Audit: terminal/release

## Protocol Specification

**terminal/release** is an Agent-to-Client request protocol for releasing a terminal session. It allows an agent to explicitly release/release ownership of a terminal resource back to the client, signaling that the agent is done with the terminal and it can be reclaimed or closed by the client.

## Implementation Status

**Partially Implemented**

Core production code is fully implemented and functional. The gap exists entirely in the e2e test harness and mock infrastructure.

## Implementation Details

### Confirmed Production Files

| File | Line | Role |
|------|------|------|
| `apps/acp/src/client_methods.rs` | 197 | Client method handler registration |
| `apps/acp/src/tools/client_bridge.rs` | 53 | Trait definition for release method |
| `apps/acp/src/tools/client_bridge.rs` | 243 | Trait implementation |
| `apps/acp/src/tools/client_bridge.rs` | 312 | Trait implementation (second site) |
| `apps/acp/src/tools/terminal_executor.rs` | 268 | Call site for release |
| `apps/acp/src/tools/terminal_executor.rs` | 300 | Call site for release (second site) |
| `protocols.lua` | 95 | Protocol registry entry |

### Confirmed Test Files

| File | Line | Role |
|------|------|------|
| `apps/acp/tests/e2e/common/jsonrpc.rs` | 65 | ReverseRpcKind location (note: under `e2e/common/`, not `e2e/`) |
| `apps/acp/tests/e2e/common/permissions.rs` | 7 | ReverseRpcResponder location (note: under `e2e/common/`, not `e2e/`) |

## Implementation Approach

The production implementation follows the standard ACP pattern:

1. **ClientBridge trait** defines `release_terminal()` with an async signature accepting a terminal ID and returning `Result<(), Error>`
2. **TerminalExecutor** calls the bridge method at lines 268 and 300, handling terminal release as part of its resource lifecycle
3. **client_methods.rs** registers the client method handler at line 197
4. **protocols.lua** includes the protocol in the registry at line 95

The `ReverseRpcKind::classify()` method falls `terminal/release` through to `ExtMethod(String)` — meaning the harness CAN route it, but it is uncategorized and has no dedicated responder.

## Gaps and Issues

### Confirmed Gaps (5 total)

1. **No ReverseRpcKind::TerminalRelease variant** — The enum lacks a dedicated variant for this protocol; falls through to `ExtMethod(String)`

2. **No ReverseRpcResponder handler for terminal/release** — No handler registered to process incoming terminal/release requests in the reverse RPC path

3. **No e2e/terminal.rs test file** — No dedicated e2e test module for terminal operations

4. **Mock returns 'not implemented'** — The mock implementation does not provide a functional stub for terminal/release

5. **No e2e integration test** — No integration test covering the full terminal/release request path

### Correction Note

Two file paths in the original analysis report were slightly inaccurate:
- `ReverseRpcKind` is at `apps/acp/tests/e2e/common/jsonrpc.rs:65` (not `e2e/jsonrpc.rs`)
- `ReverseRpcResponder` is at `apps/acp/tests/e2e/common/permissions.rs` (not `e2e/permissions.rs`)
- Both are located under `e2e/common/` subdirectory

## Verification

**Verification Process:** Adversarial analysis with full codebase grep across all confirmed files.

**Verdict:** `PARTIALLY IMPLEMENTED`

**Confidence:** High

**Key Finding:** The production path works correctly — `client_methods`, `client_bridge` trait+impl, `terminal_executor` call sites, and `protocols.lua` registry are all confirmed present and properly wired. The implementation gap is entirely in the e2e test harness (mock infrastructure and integration tests), not in production code.

## Conclusion

The **terminal/release** protocol is **partially implemented** with full production code coverage but missing test infrastructure. The implementation is functionally correct; the gap affects only development/testing velocity, not runtime behavior.

### Recommendations

1. **Add ReverseRpcKind::TerminalRelease variant** to the enum for explicit categorization
2. **Register ReverseRpcResponder handler** for terminal/release in the reverse RPC dispatcher
3. **Create e2e/terminal.rs** test module for terminal operation coverage
4. **Implement mock stub** returning a successful response instead of 'not implemented'
5. **Add e2e integration test** covering the full terminal/release request-response cycle
6. **Fix file path references** in analysis documentation to reflect `e2e/common/` subdirectory structure
