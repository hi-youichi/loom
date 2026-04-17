type Args = Record<string, unknown>

const TOOL_DISPLAY_NAMES: Record<string, string> = {
  read: 'Read',
  edit: 'Edit',
  write_file: 'Write',
  delete_file: 'Delete',
  move_file: 'Move',
  create_dir: 'Create Dir',
  apply_patch: 'Patch',
  multiedit: 'Multi-Edit',
  glob: 'Glob',
  grep: 'Grep',
  ls: 'List',
  bash: 'Bash',
  powershell: 'PowerShell',
  web_fetcher: 'Fetch',
  websearch: 'Search',
  remember: 'Remember',
  recall: 'Recall',
  search_memories: 'Search Memory',
  list_memories: 'List Memory',
  todo_read: 'Todo Read',
  todo_write: 'Todo Write',
  skill: 'Skill',
  lsp: 'LSP',
  invoke_agent: 'Agent',
  get_recent_messages: 'History',
  batch: 'Batch',
  twitter_search: 'Twitter',
  telegram_send_message: 'Telegram',
  telegram_send_document: 'Send Doc',
  telegram_send_poll: 'Poll',
}

const TITLE_PARAMS: Record<string, (args: Args) => string | null> = {
  read: (a) => str(a, 'path'),
  edit: (a) => str(a, 'path'),
  write_file: (a) => str(a, 'path'),
  delete_file: (a) => str(a, 'path'),
  move_file: (a) => {
    const s = str(a, 'source')
    const t = str(a, 'target')
    return s && t ? `${s} → ${t}` : s
  },
  create_dir: (a) => str(a, 'path'),
  apply_patch: (a) => str(a, 'path'),
  multiedit: (a) => str(a, 'path'),
  glob: (a) => str(a, 'pattern'),
  grep: (a) => {
    const p = str(a, 'pattern')
    const path = str(a, 'path')
    return p && path ? `${p} in ${path}` : p
  },
  ls: (a) => str(a, 'path'),
  bash: (a) => truncate(str(a, 'command') || '', 60),
  powershell: (a) => truncate(str(a, 'command') || '', 60),
  web_fetcher: (a) => str(a, 'url'),
  websearch: (a) => str(a, 'query'),
  remember: (a) => str(a, 'key'),
  recall: (a) => str(a, 'key'),
  search_memories: (a) => str(a, 'query'),
  list_memories: () => null,
  todo_read: () => null,
  todo_write: () => null,
  skill: (a) => str(a, 'name'),
  lsp: (a) => `${str(a, 'action') || ''} ${str(a, 'file_path') || ''}`.trim() || null,
  invoke_agent: (a) => truncate(str(a, 'task') || '', 60),
  get_recent_messages: () => null,
  batch: () => null,
  twitter_search: (a) => str(a, 'query'),
  telegram_send_message: () => null,
  telegram_send_document: () => null,
  telegram_send_poll: () => null,
}

function str(args: Args, key: string): string | null {
  const v = args[key]
  if (typeof v === 'string' && v.length > 0) return v
  return null
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return s.slice(0, max) + '\u2026'
}

export function getToolDisplayName(name: string): string {
  return TOOL_DISPLAY_NAMES[name] || name
}

export function extractToolTitle(name: string, args: Args): string | null {
  const fn = TITLE_PARAMS[name]
  if (!fn) return null
  return fn(args)
}
