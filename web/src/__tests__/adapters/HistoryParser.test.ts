/**
 * HistoryParser 测试
 * 验证扁平 (role, content) 历史消息到 UIMessageItemProps[] 的转换
 */
import { describe, it, expect } from 'vitest'
import { parseHistoryMessages } from '../../adapters/HistoryParser'
import type { UserMessageItem } from '../../services/userMessages'

describe('HistoryParser', () => {
  describe('parseHistoryMessages', () => {
    it('should handle empty input', () => {
      const result = parseHistoryMessages([])
      expect(result).toEqual([])
    })

    it('should convert plain user messages', () => {
      const items: UserMessageItem[] = [
        { role: 'user', content: 'Hello, world!' },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      expect(result[0].sender).toBe('user')
      expect(result[0].content).toEqual([
        { type: 'text', text: 'Hello, world!', format: 'plain' },
      ])
    })

    it('should convert plain assistant messages', () => {
      const items: UserMessageItem[] = [
        { role: 'assistant', content: 'Hi there!' },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      expect(result[0].sender).toBe('assistant')
      expect(result[0].content).toEqual([
        { type: 'text', text: 'Hi there!', format: 'plain' },
      ])
    })

    it('should skip system messages', () => {
      const items: UserMessageItem[] = [
        { role: 'system', content: 'You are helpful' },
        { role: 'user', content: 'Hello' },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      expect(result[0].sender).toBe('user')
    })

    it('should skip tool messages (they are embedded in assistant messages)', () => {
      const items: UserMessageItem[] = [
        { role: 'user', content: 'Hello' },
        { role: 'tool', content: '{"tool_call_id":"call_123","content":"file content"}' },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      expect(result[0].sender).toBe('user')
    })

    it('should parse assistant messages with tool_calls', () => {
      const items: UserMessageItem[] = [
        { role: 'user', content: 'Read file.ts' },
        {
          role: 'assistant',
          content: JSON.stringify({
            content: 'Let me read that file.',
            tool_calls: [
              {
                id: 'call_abc123',
                name: 'read',
                arguments: '{"path":"file.ts"}',
              },
            ],
          }),
        },
        {
          role: 'tool',
          content: JSON.stringify({
            tool_call_id: 'call_abc123',
            content: 'file content here',
          }),
        },
        {
          role: 'assistant',
          content: 'Here is the file content.',
        },
      ]
      const result = parseHistoryMessages(items)

      // user, assistant(with tools), assistant(text) = 3
      expect(result).toHaveLength(3)

      // 第一个 assistant 消息应该有文本 + 工具块
      const assistantWithTools = result[1]
      expect(assistantWithTools.sender).toBe('assistant')
      expect(assistantWithTools.content).toHaveLength(2)
      expect(assistantWithTools.content[0].type).toBe('text')
      if (assistantWithTools.content[0].type === 'text') {
        expect(assistantWithTools.content[0].text).toBe('Let me read that file.')
      }
      expect(assistantWithTools.content[1].type).toBe('tool')
      if (assistantWithTools.content[1].type === 'tool') {
        expect(assistantWithTools.content[1].name).toBe('read')
        expect(assistantWithTools.content[1].id).toBe('call_abc123')
        expect(assistantWithTools.content[1].status).toBe('success')
        expect(assistantWithTools.content[1].outputText).toBe('file content here')
      }

      // 最后一个 assistant 消息是纯文本
      const lastAssistant = result[2]
      expect(lastAssistant.sender).toBe('assistant')
      expect(lastAssistant.content).toHaveLength(1)
      expect(lastAssistant.content[0].type).toBe('text')
    })

    it('should handle assistant with tool_calls but no matching tool result', () => {
      const items: UserMessageItem[] = [
        {
          role: 'assistant',
          content: JSON.stringify({
            content: '',
            tool_calls: [
              {
                id: 'call_missing',
                name: 'bash',
                arguments: '{"command":"ls"}',
              },
            ],
          }),
        },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      expect(result[0].content).toHaveLength(1)
      expect(result[0].content[0].type).toBe('tool')
      if (result[0].content[0].type === 'tool') {
        expect(result[0].content[0].outputText).toBe('')
        expect(result[0].content[0].name).toBe('bash')
      }
    })

    it('should handle multiple tool_calls in a single assistant message', () => {
      const items: UserMessageItem[] = [
        {
          role: 'assistant',
          content: JSON.stringify({
            content: 'Reading multiple files.',
            tool_calls: [
              { id: 'call_1', name: 'read', arguments: '{"path":"a.ts"}' },
              { id: 'call_2', name: 'read', arguments: '{"path":"b.ts"}' },
            ],
          }),
        },
        {
          role: 'tool',
          content: JSON.stringify({ tool_call_id: 'call_1', content: 'content A' }),
        },
        {
          role: 'tool',
          content: JSON.stringify({ tool_call_id: 'call_2', content: 'content B' }),
        },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)

      const msg = result[0]
      expect(msg.content).toHaveLength(3) // 1 text + 2 tools
      expect(msg.content[0].type).toBe('text')
      expect(msg.content[1].type).toBe('tool')
      expect(msg.content[2].type).toBe('tool')

      if (msg.content[1].type === 'tool' && msg.content[2].type === 'tool') {
        expect(msg.content[1].outputText).toBe('content A')
        expect(msg.content[2].outputText).toBe('content B')
      }
    })

    it('should handle assistant with empty content but tool_calls', () => {
      const items: UserMessageItem[] = [
        {
          role: 'assistant',
          content: JSON.stringify({
            content: '',
            tool_calls: [
              { id: 'call_1', name: 'edit', arguments: '{"path":"f.ts"}' },
            ],
          }),
        },
        {
          role: 'tool',
          content: JSON.stringify({ tool_call_id: 'call_1', content: 'applied' }),
        },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      // 空文本被跳过，只有工具块
      expect(result[0].content).toHaveLength(1)
      expect(result[0].content[0].type).toBe('tool')
    })

    it('should handle assistant content that starts with { but is not JSON', () => {
      const items: UserMessageItem[] = [
        {
          role: 'assistant',
          content: '{ this is not JSON but just text with a brace',
        },
      ]
      const result = parseHistoryMessages(items)
      expect(result).toHaveLength(1)
      expect(result[0].content[0].type).toBe('text')
      if (result[0].content[0].type === 'text') {
        expect(result[0].content[0].text).toBe('{ this is not JSON but just text with a brace')
      }
    })

    it('should correctly parse arguments as JSON for display', () => {
      const items: UserMessageItem[] = [
        {
          role: 'assistant',
          content: JSON.stringify({
            content: '',
            tool_calls: [
              {
                id: 'call_1',
                name: 'read',
                arguments: '{"path":"src/index.ts","offset":10}',
              },
            ],
          }),
        },
        {
          role: 'tool',
          content: JSON.stringify({ tool_call_id: 'call_1', content: 'file...' }),
        },
      ]
      const result = parseHistoryMessages(items)
      const tool = result[0].content[0]
      if (tool.type === 'tool') {
        // arguments 应该被 pretty-print
        expect(tool.argumentsText).toContain('"path"')
        expect(tool.argumentsText).toContain('"src/index.ts"')
      }
    })

    it('should handle full conversation flow: user -> assistant(tool) -> assistant(text)', () => {
      const items: UserMessageItem[] = [
        { role: 'user', content: 'What is in config.toml?' },
        {
          role: 'assistant',
          content: JSON.stringify({
            content: 'Let me check.',
            tool_calls: [
              { id: 'call_r1', name: 'read', arguments: '{"path":"config.toml"}' },
            ],
          }),
        },
        {
          role: 'tool',
          content: JSON.stringify({ tool_call_id: 'call_r1', content: 'Tool read result:\n 1\t name = "test"' }),
        },
        {
          role: 'assistant',
          content: 'The config file contains a test configuration.',
        },
      ]
      const result = parseHistoryMessages(items)

      expect(result).toHaveLength(3)

      // user
      expect(result[0].sender).toBe('user')

      // assistant with tool
      expect(result[1].sender).toBe('assistant')
      expect(result[1].content).toHaveLength(2) // text + tool
      expect(result[1].content[0].type).toBe('text')
      expect(result[1].content[1].type).toBe('tool')

      // final assistant text
      expect(result[2].sender).toBe('assistant')
      expect(result[2].content).toHaveLength(1)
      expect(result[2].content[0].type).toBe('text')
    })
  })
})
