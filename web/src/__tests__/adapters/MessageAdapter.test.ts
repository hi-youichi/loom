/**
 * MessageAdapter 测试
 * 覆盖率目标: 95%+
 */

import { describe, it, expect } from 'vitest'
import { MessageAdapter } from '../adapters/MessageAdapter'
import type { 
  LoomUserEvent, 
  LoomAssistantTextEvent, 
  LoomAssistantToolEvent 
} from '../types/protocol/loom'

describe('MessageAdapter', () => {
  describe('toUI - 单个事件转换', () => {
    it('应该正确转换用户事件', () => {
      const userEvent: LoomUserEvent = {
        type: 'user',
        id: 'user-1',
        createdAt: '2024-01-01T10:00:00Z',
        text: 'Hello, world!'
      }

      const result = MessageAdapter.toUI(userEvent)

      expect(result.id).toBe('user-1')
      expect(result.sender).toBe('user')
      expect(result.timestamp).toBe('2024-01-01T10:00:00Z')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('text')
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe('Hello, world!')
        expect(result.content[0].format).toBe('plain')
      }
    })

    it('应该正确转换助手文本事件', () => {
      const assistantEvent: LoomAssistantTextEvent = {
        type: 'assistant_text',
        id: 'assistant-1',
        createdAt: '2024-01-01T10:01:00Z',
        text: 'Hello! How can I help you?'
      }

      const result = MessageAdapter.toUI(assistantEvent)

      expect(result.id).toBe('assistant-1')
      expect(result.sender).toBe('assistant')
      expect(result.timestamp).toBe('2024-01-01T10:01:00Z')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('text')
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe('Hello! How can I help you?')
      }
    })

    it('应该正确转换助手工具事件', () => {
      const toolEvent: LoomAssistantToolEvent = {
        type: 'assistant_tool',
        id: 'tool-1',
        createdAt: '2024-01-01T10:02:00Z',
        callId: 'call-123',
        name: 'bash',
        status: 'done',
        argumentsText: '{"command": "ls"}',
        outputText: 'file1.txt\nfile2.txt',
        resultText: 'Success',
        isError: false
      }

      const result = MessageAdapter.toUI(toolEvent)

      expect(result.id).toBe('tool-1')
      expect(result.sender).toBe('assistant')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('tool')
      if (result.content[0].type === 'tool') {
        expect(result.content[0].id).toBe('call-123')
        expect(result.content[0].name).toBe('bash')
        expect(result.content[0].status).toBe('success')
        expect(result.content[0].argumentsText).toBe('{"command": "ls"}')
        expect(result.content[0].outputText).toBe('file1.txt\nfile2.txt')
        expect(result.content[0].resultText).toBe('Success')
        expect(result.content[0].isError).toBe(false)
      }
    })

    it('应该正确转换错误状态的工具事件', () => {
      const toolEvent: LoomAssistantToolEvent = {
        type: 'assistant_tool',
        id: 'tool-2',
        createdAt: '2024-01-01T10:03:00Z',
        callId: 'call-456',
        name: 'bash',
        status: 'error',
        argumentsText: '{"command": "invalid"}',
        outputText: '',
        resultText: 'Command not found',
        isError: true
      }

      const result = MessageAdapter.toUI(toolEvent)

      expect(result.content[0].type).toBe('tool')
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('error')
        expect(result.content[0].isError).toBe(true)
        expect(result.content[0].resultText).toBe('Command not found')
      }
    })

    it('应该正确映射不同的工具状态', () => {
      const statuses: Array<LoomAssistantToolEvent['status']> = [
        'queued',
        'running',
        'done',
        'error',
        'approval_required'
      ]

      const expectedMapping = {
        'queued': 'pending',
        'running': 'running',
        'done': 'success',
        'error': 'error',
        'approval_required': 'pending'
      }

      statuses.forEach(status => {
        const toolEvent: LoomAssistantToolEvent = {
          type: 'assistant_tool',
          id: `tool-${status}`,
          createdAt: '2024-01-01T10:00:00Z',
          callId: 'call-test',
          name: 'test',
          status,
          argumentsText: '',
          outputText: '',
          resultText: '',
          isError: false
        }

        const result = MessageAdapter.toUI(toolEvent)
        expect(result.content[0].type).toBe('tool')
        if (result.content[0].type === 'tool') {
          expect(result.content[0].status).toBe(expectedMapping[status])
        }
      })
    })
  })

  describe('toUIList - 批量转换', () => {
    it('应该正确转换事件列表', () => {
      const events = [
        {
          type: 'user' as const,
          id: '1',
          createdAt: '2024-01-01T10:00:00Z',
          text: 'Hello'
        },
        {
          type: 'assistant_text' as const,
          id: '2',
          createdAt: '2024-01-01T10:01:00Z',
          text: 'Hi there!'
        }
      ]

      const results = MessageAdapter.toUIList(events)

      expect(results).toHaveLength(2)
      expect(results[0].sender).toBe('user')
      expect(results[1].sender).toBe('assistant')
    })

    it('应该处理空列表', () => {
      const results = MessageAdapter.toUIList([])
      expect(results).toHaveLength(0)
    })
  })

  describe('mergeEvents - 事件合并', () => {
    it('应该合并连续的助手事件', () => {
      const events = [
        {
          type: 'user' as const,
          id: '1',
          createdAt: '2024-01-01T10:00:00Z',
          text: 'Hello'
        },
        {
          type: 'assistant_text' as const,
          id: '2',
          createdAt: '2024-01-01T10:01:00Z',
          text: 'Hi! '
        },
        {
          type: 'assistant_tool' as const,
          id: '3',
          createdAt: '2024-01-01T10:02:00Z',
          callId: 'call-1',
          name: 'bash',
          status: 'done' as const,
          argumentsText: '',
          outputText: 'output',
          resultText: '',
          isError: false
        },
        {
          type: 'assistant_text' as const,
          id: '4',
          createdAt: '2024-01-01T10:03:00Z',
          text: 'Done!'
        }
      ]

      const results = MessageAdapter.mergeEvents(events)

      expect(results).toHaveLength(2)
      expect(results[0].sender).toBe('user')
      expect(results[0].content).toHaveLength(1)
      
      expect(results[1].sender).toBe('assistant')
      expect(results[1].content).toHaveLength(3) // text + tool + text
    })

    it('应该将每个用户消息作为独立消息', () => {
      const events = [
        {
          type: 'user' as const,
          id: '1',
          createdAt: '2024-01-01T10:00:00Z',
          text: 'First'
        },
        {
          type: 'user' as const,
          id: '2',
          createdAt: '2024-01-01T10:01:00Z',
          text: 'Second'
        }
      ]

      const results = MessageAdapter.mergeEvents(events)

      expect(results).toHaveLength(2)
      expect(results[0].id).toBe('1')
      expect(results[1].id).toBe('2')
    })

    it('应该处理空事件列表', () => {
      const results = MessageAdapter.mergeEvents([])
      expect(results).toHaveLength(0)
    })
  })

  describe('updateMessage - 消息更新', () => {
    it('应该追加文本到最后一个文本块', () => {
      const message = {
        id: '1',
        sender: 'assistant' as const,
        timestamp: '2024-01-01T10:00:00Z',
        content: [
          { type: 'text' as const, text: 'Hello', format: 'plain' as const }
        ]
      }

      const event = {
        type: 'assistant_text' as const,
        id: '2',
        createdAt: '2024-01-01T10:01:00Z',
        text: ' World!'
      }

      const updated = MessageAdapter.updateMessage(message, event)

      expect(updated.content).toHaveLength(1)
      if (updated.content[0].type === 'text') {
        expect(updated.content[0].text).toBe('Hello World!')
      }
    })

    it('应该添加新的文本块如果没有文本块', () => {
      const message = {
        id: '1',
        sender: 'assistant' as const,
        timestamp: '2024-01-01T10:00:00Z',
        content: [
          { 
            type: 'tool' as const, 
            id: 'tool-1',
            name: 'bash',
            status: 'success' as const,
            argumentsText: '',
            outputText: '',
            resultText: '',
            isError: false
          }
        ]
      }

      const event = {
        type: 'assistant_text' as const,
        id: '2',
        createdAt: '2024-01-01T10:01:00Z',
        text: 'Done!'
      }

      const updated = MessageAdapter.updateMessage(message, event)

      expect(updated.content).toHaveLength(2)
      expect(updated.content[1].type).toBe('text')
    })

    it('应该添加工具块', () => {
      const message = {
        id: '1',
        sender: 'assistant' as const,
        timestamp: '2024-01-01T10:00:00Z',
        content: []
      }

      const event = {
        type: 'assistant_tool' as const,
        id: '2',
        createdAt: '2024-01-01T10:01:00Z',
        callId: 'call-1',
        name: 'bash',
        status: 'running' as const,
        argumentsText: '{"command": "ls"}',
        outputText: '',
        resultText: '',
        isError: false
      }

      const updated = MessageAdapter.updateMessage(message, event)

      expect(updated.content).toHaveLength(1)
      expect(updated.content[0].type).toBe('tool')
    })
  })

  describe('边界情况', () => {
    it('应该处理空文本', () => {
      const event = {
        type: 'user' as const,
        id: '1',
        createdAt: '2024-01-01T10:00:00Z',
        text: ''
      }

      const result = MessageAdapter.toUI(event)
      
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe('')
      }
    })

    it('应该处理特殊字符', () => {
      const event = {
        type: 'user' as const,
        id: '1',
        createdAt: '2024-01-01T10:00:00Z',
        text: 'Hello\nWorld\t🎉'
      }

      const result = MessageAdapter.toUI(event)
      
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe('Hello\nWorld\t🎉')
      }
    })

    it('应该处理非常长的文本', () => {
      const longText = 'A'.repeat(10000)
      const event = {
        type: 'user' as const,
        id: '1',
        createdAt: '2024-01-01T10:00:00Z',
        text: longText
      }

      const result = MessageAdapter.toUI(event)
      
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe(longText)
        expect(result.content[0].text).toHaveLength(10000)
      }
    })
  })
})
