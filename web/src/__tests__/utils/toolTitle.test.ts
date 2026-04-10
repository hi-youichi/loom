import { describe, it, expect } from 'vitest'
import { getToolDisplayName, extractToolTitle } from '../../utils/toolTitle'

describe('toolTitle', () => {
  describe('getToolDisplayName', () => {
    it('returns display name for known tools', () => {
      expect(getToolDisplayName('read')).toBe('Read')
      expect(getToolDisplayName('edit')).toBe('Edit')
      expect(getToolDisplayName('write_file')).toBe('Write')
      expect(getToolDisplayName('delete_file')).toBe('Delete')
      expect(getToolDisplayName('move_file')).toBe('Move')
      expect(getToolDisplayName('create_dir')).toBe('Create Dir')
      expect(getToolDisplayName('apply_patch')).toBe('Patch')
      expect(getToolDisplayName('multiedit')).toBe('Multi-Edit')
      expect(getToolDisplayName('glob')).toBe('Glob')
      expect(getToolDisplayName('grep')).toBe('Grep')
      expect(getToolDisplayName('ls')).toBe('List')
      expect(getToolDisplayName('bash')).toBe('Bash')
      expect(getToolDisplayName('powershell')).toBe('PowerShell')
      expect(getToolDisplayName('web_fetcher')).toBe('Fetch')
      expect(getToolDisplayName('websearch')).toBe('Search')
      expect(getToolDisplayName('remember')).toBe('Remember')
      expect(getToolDisplayName('recall')).toBe('Recall')
      expect(getToolDisplayName('search_memories')).toBe('Search Memory')
      expect(getToolDisplayName('list_memories')).toBe('List Memory')
      expect(getToolDisplayName('todo_read')).toBe('Todo Read')
      expect(getToolDisplayName('todo_write')).toBe('Todo Write')
      expect(getToolDisplayName('skill')).toBe('Skill')
      expect(getToolDisplayName('lsp')).toBe('LSP')
      expect(getToolDisplayName('invoke_agent')).toBe('Agent')
      expect(getToolDisplayName('get_recent_messages')).toBe('History')
      expect(getToolDisplayName('batch')).toBe('Batch')
      expect(getToolDisplayName('twitter_search')).toBe('Twitter')
      expect(getToolDisplayName('telegram_send_message')).toBe('Telegram')
      expect(getToolDisplayName('telegram_send_document')).toBe('Send Doc')
      expect(getToolDisplayName('telegram_send_poll')).toBe('Poll')
    })

    it('returns the raw name for unknown tools', () => {
      expect(getToolDisplayName('unknown_tool')).toBe('unknown_tool')
      expect(getToolDisplayName('custom')).toBe('custom')
    })
  })

  describe('extractToolTitle', () => {
    it('extracts path for read', () => {
      expect(extractToolTitle('read', { path: 'src/main.ts' })).toBe('src/main.ts')
    })

    it('extracts path for edit', () => {
      expect(extractToolTitle('edit', { path: 'src/app.ts' })).toBe('src/app.ts')
    })

    it('extracts path for write_file', () => {
      expect(extractToolTitle('write_file', { path: 'out.txt' })).toBe('out.txt')
    })

    it('extracts path for delete_file', () => {
      expect(extractToolTitle('delete_file', { path: 'old.txt' })).toBe('old.txt')
    })

    it('extracts source → target for move_file', () => {
      expect(extractToolTitle('move_file', { source: 'a.ts', target: 'b.ts' })).toBe('a.ts → b.ts')
    })

    it('extracts only source for move_file when no target', () => {
      expect(extractToolTitle('move_file', { source: 'a.ts' })).toBe('a.ts')
    })

    it('extracts path for create_dir', () => {
      expect(extractToolTitle('create_dir', { path: 'src/new' })).toBe('src/new')
    })

    it('extracts path for apply_patch', () => {
      expect(extractToolTitle('apply_patch', { path: 'patch.ts' })).toBe('patch.ts')
    })

    it('extracts path for multiedit', () => {
      expect(extractToolTitle('multiedit', { path: 'multi.ts' })).toBe('multi.ts')
    })

    it('extracts pattern for glob', () => {
      expect(extractToolTitle('glob', { pattern: '**/*.ts' })).toBe('**/*.ts')
    })

    it('extracts pattern in path for grep', () => {
      expect(extractToolTitle('grep', { pattern: 'TODO', path: 'src' })).toBe('TODO in src')
    })

    it('extracts only pattern for grep without path', () => {
      expect(extractToolTitle('grep', { pattern: 'TODO' })).toBe('TODO')
    })

    it('extracts path for ls', () => {
      expect(extractToolTitle('ls', { path: 'src' })).toBe('src')
    })

    it('extracts truncated command for bash', () => {
      const longCmd = 'a'.repeat(100)
      const result = extractToolTitle('bash', { command: longCmd })
      expect(result).toBe(longCmd.slice(0, 60) + '\u2026')
    })

    it('extracts short command for bash', () => {
      expect(extractToolTitle('bash', { command: 'ls' })).toBe('ls')
    })

    it('extracts truncated command for powershell', () => {
      const longCmd = 'Get-ChildItem ' + 'x'.repeat(100)
      const result = extractToolTitle('powershell', { command: longCmd })
      expect(result!.length).toBeLessThanOrEqual(61) // 60 + ellipsis
    })

    it('extracts url for web_fetcher', () => {
      expect(extractToolTitle('web_fetcher', { url: 'https://example.com' })).toBe('https://example.com')
    })

    it('extracts query for websearch', () => {
      expect(extractToolTitle('websearch', { query: 'test query' })).toBe('test query')
    })

    it('extracts key for remember', () => {
      expect(extractToolTitle('remember', { key: 'my-key' })).toBe('my-key')
    })

    it('extracts key for recall', () => {
      expect(extractToolTitle('recall', { key: 'my-key' })).toBe('my-key')
    })

    it('extracts query for search_memories', () => {
      expect(extractToolTitle('search_memories', { query: 'test' })).toBe('test')
    })

    it('returns null for list_memories', () => {
      expect(extractToolTitle('list_memories', {})).toBeNull()
    })

    it('returns null for todo_read', () => {
      expect(extractToolTitle('todo_read', {})).toBeNull()
    })

    it('returns null for todo_write', () => {
      expect(extractToolTitle('todo_write', {})).toBeNull()
    })

    it('extracts name for skill', () => {
      expect(extractToolTitle('skill', { name: 'rust-architecture' })).toBe('rust-architecture')
    })

    it('extracts action and file_path for lsp', () => {
      expect(extractToolTitle('lsp', { action: 'completion', file_path: 'a.ts' })).toBe('completion a.ts')
    })

    it('returns null for lsp with no args', () => {
      expect(extractToolTitle('lsp', {})).toBeNull()
    })

    it('extracts truncated task for invoke_agent', () => {
      const longTask = 't'.repeat(100)
      const result = extractToolTitle('invoke_agent', { task: longTask })
      expect(result).toBe(longTask.slice(0, 60) + '\u2026')
    })

    it('returns null for get_recent_messages', () => {
      expect(extractToolTitle('get_recent_messages', {})).toBeNull()
    })

    it('returns null for batch', () => {
      expect(extractToolTitle('batch', {})).toBeNull()
    })

    it('extracts query for twitter_search', () => {
      expect(extractToolTitle('twitter_search', { query: '#test' })).toBe('#test')
    })

    it('returns null for telegram_send_message', () => {
      expect(extractToolTitle('telegram_send_message', {})).toBeNull()
    })

    it('returns null for telegram_send_document', () => {
      expect(extractToolTitle('telegram_send_document', {})).toBeNull()
    })

    it('returns null for telegram_send_poll', () => {
      expect(extractToolTitle('telegram_send_poll', {})).toBeNull()
    })

    it('returns null for unknown tool', () => {
      expect(extractToolTitle('unknown_tool', { foo: 'bar' })).toBeNull()
    })

    it('returns null when path value is empty string', () => {
      expect(extractToolTitle('read', { path: '' })).toBeNull()
    })

    it('returns null when path value is not a string', () => {
      expect(extractToolTitle('read', { path: 123 })).toBeNull()
    })
  })
})
