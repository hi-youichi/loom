# Configuration System

Loom uses **HelveConfig** for product-level options and **ReactBuildConfig** for runner construction. This document covers config structure, profile management, and feature flags.

## HelveConfig structure

**HelveConfig** holds product-semantic fields:

- **working_folder**: Optional path for file tools and prompt assembly.
- **thread_id**, **user_id**: Conversation and user identity for checkpointer and store.
- **approval_policy**: When set, tools that require approval (e.g. destructive file ops) trigger an interrupt; see **ApprovalPolicy**, **tools_requiring_approval**.
- **role_setting**: Persona text (e.g. from instructions.md) prepended to the system prompt.
- **agents_md**: Project-level rules (e.g. from AGENTS.md); appended after role_setting.
- **skills_prompt**: Optional skills summary injected into the system prompt.
- **system_prompt_override**: When set, used as the full system prompt (no assembly).

**to_react_build_config(helve, base)** merges **HelveConfig** with a base **ReactBuildConfig**: product fields from helve when set, otherwise from base; system prompt is assembled from override, or role_setting + agents_md + base content (with workdir/approval assembly when applicable).

## Profile management

- **Profiles** (e.g. from AGENTS.md or profile files) list available agents. **resolve_profile**, **list_available_profiles** return **AgentProfile** / **ProfileSummary**.
- **ReactBuildConfig** can be built from env (**from_env**), from a profile (**build_config_from_profile**), or by merging **HelveConfig** with **to_react_build_config**.
- **ReactBuildConfig** includes: model, system_prompt, thread_id, user_id, db_path, mcp_* / openai_* keys, dry_run, approval_policy, compaction_config, etc.

## Environment-based configuration

- **ReactBuildConfig::from_env** reads environment variables (e.g. OPENAI_API_KEY, OPENAI_BASE_URL, MODEL, LLM_PROVIDER, thread_id, user_id, db_path). Used as the base when merging with **HelveConfig** or profile overrides.

## Feature flags (lance backend)

- **lance**: Enables LanceDB-backed **LanceStore** for vector search in long-term memory. When disabled, vector store backends are not compiled. Other backends (e.g. **SqliteStore**, **InMemoryStore**) do not require this feature.

## Summary

| Topic | Notes |
|-------|--------|
| HelveConfig | working_folder, thread_id, user_id, approval_policy, role_setting, agents_md, system_prompt_override |
| to_react_build_config | Merge helve + base; assemble system prompt |
| Profiles | resolve_profile, build_config_from_profile |
| Env | ReactBuildConfig::from_env |
| Feature | lance for LanceStore |

Next: [Visualization](visualization.md) for graph export.
