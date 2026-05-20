import type { SidebarsConfig } from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docs: [
    {
      type: 'category',
      label: '入门',
      collapsed: false,
      items: [
        'getting-started/overview',
        'getting-started/quickstart',
        'getting-started/concepts',
      ],
    },
    {
      type: 'category',
      label: '核心概念',
      items: [
        'core/state-graph',
        'core/node-middleware',
        'core/react',
        'core/dup',
        'core/tot',
        'core/got',
        'core/llm-client',
      ],
    },
    {
      type: 'category',
      label: '工具与集成',
      items: [
        'tools/tool-system',
        'tools/shell-background-timeout',
        'tools/codex-shell-execution',
        'tools/mcp',
        'skills',
      ],
    },
    {
      type: 'category',
      label: '记忆与存储',
      items: [
        'memory/checkpointer-store',
        'memory/channels',
      ],
    },
    {
      type: 'category',
      label: '流式与可观测',
      items: ['streaming/streaming'],
    },
    {
      type: 'category',
      label: '部署与运维',
      items: [
        'deployment/cli',
        'deployment/cli-json-output',
        'deployment/cli-session-cat',
        'deployment/troubleshooting',
      ],
    },
    {
      type: 'category',
      label: '进阶',
      items: ['advanced/api-reference'],
    },
    {
      type: 'category',
      label: '设计文档',
      items: [
        'design/session-dump',
        'design/claude-code-compat',
        'codex-goal-feature',
        'design/session-cat-tasks',
        'design/task-integration',
        'design/meta-agent-architecture',
        'design/goal-ralph-loop',
        'design/goal-external-loop',
      ],
    },
    {
      type: 'category',
      label: '架构决策记录',
      items: [
        'adr/act-node-refactoring',
        'adr/claude-code-schema',
      ],
    },
    {
      type: 'category',
      label: 'Claude Code 兼容',
      items: [
        'reference/claude-code-json-protocol',
        'reference/claude-code-schema-types',
        'reference/codex-reference',
        'reference/codex-event-protocol',
        'reference/codex-error-handling',
      ],
    },
  ],
};

export default sidebars;
