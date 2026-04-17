/**
 * MessageAdapter 测试
 * 覆盖率目标: 95%+
 */

import { describe, it, expect } from 'vitest'
import { MessageAdapter } from '../../adapters/MessageAdapter'
import type { Message, TextBlock, ToolBlock } from '../../types/chat'

describe('MessageAdapter', () => {
  describe('toUI - 单个消息转换', () => {
    it('应该正确转换用户文本消息', () => {
      const textBlock: TextBlock = {
        id: 'block-1',
        type: 'text',
        text: 'Hello, world!'
      }
      
      const userMessage: Message = {
        id: 'user-1',
        role: 'user',
        createdAt: '2024-01-01T10:00:00Z',
        blocks: [textBlock]
      }

      const result = MessageAdapter.toUI(userMessage)

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

    it('应该正确转换助手文本消息', () => {
      const textBlock: TextBlock = {
        id: 'block-1',
        type: 'text',
        text: 'This is assistant response'
      }
      
      const assistantMessage: Message = {
        id: 'assistant-1',
        role: 'assistant',
        createdAt: '2024-01-01T10:01:00Z',
        blocks: [textBlock]
      }

      const result = MessageAdapter.toUI(assistantMessage)

      expect(result.id).toBe('assistant-1')
      expect(result.sender).toBe('assistant')
      expect(result.timestamp).toBe('2024-01-01T10:01:00Z')
      expect(result.content).toHaveLength(1)
    })

    it('应该正确转换包含工具调用的消息', () => {
      const toolBlock: ToolBlock = {
        id: 'tool-1',
        type: 'tool',
        callId: 'call-123',
        name: 'get_weather',
        status: 'done',
        argumentsText: '{"location": "Beijing"}',
        outputText: '{"temperature": 25}',
        resultText: 'Temperature: 25°C',
        isError: false
      }
      
      const toolMessage: Message = {
        id: 'msg-tool-1',
        role: 'assistant',
        createdAt: '2024-01-01T10:02:00Z',
        blocks: [toolBlock]
      }

      const result = MessageAdapter.toUI(toolMessage)

      expect(result.id).toBe('msg-tool-1')
      expect(result.sender).toBe('assistant')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('tool')
    })

    it('应该正确转换多块消息', () => {
      const blocks: Array<TextBlock | ToolBlock> = [
        { id: 'b1', type: 'text', text: 'Let me check' },
        { id: 'b2', type: 'text', text: 'the weather' }
      ]
      
      const multiBlockMessage: Message = {
        id: 'multi-1',
        role: 'assistant',
        createdAt: '2024-01-01T10:03:00Z',
        blocks
      }

      const result = MessageAdapter.toUI(multiBlockMessage)

      expect(result.content).toHaveLength(2)
      expect(result.content[0].type).toBe('text')
      expect(result.content[1].type).toBe('text')
    })
  })

  describe('toUIList - 批量转换', () => {
    it('应该正确转换消息列表', () => {
      const messages: Message[] = [
        {
          id: 'msg-1',
          role: 'user',
          createdAt: '2024-01-01T10:00:00Z',
          blocks: [{ id: 'b1', type: 'text', text: 'Hi' }]
        },
        {
          id: 'msg-2',
          role: 'assistant',
          createdAt: '2024-01-01T10:01:00Z',
          blocks: [{ id: 'b2', type: 'text', text: 'Hello!' }]
        }
      ]

      const results = MessageAdapter.toUIList(messages)

      expect(results).toHaveLength(2)
      expect(results[0].sender).toBe('user')
      expect(results[1].sender).toBe('assistant')
    })

    it('应该处理空列表', () => {
      const results = MessageAdapter.toUIList([])
      expect(results).toHaveLength(0)
    })
  })

  describe('工具状态映射', () => {
    it('应该将 approval_required 状态映射为 queued', () => {
      const toolBlock: ToolBlock = {
        id: 'tool-1',
        type: 'tool',
        callId: 'call-123',
        name: 'dangerous_tool',
        status: 'approval_required',
        argumentsText: '{}',
        outputText: '',
        resultText: '',
        isError: false
      }
      
      const message: Message = {
        id: 'msg-1',
        role: 'assistant',
        createdAt: '2024-01-01T10:00:00Z',
        blocks: [toolBlock]
      }

      const result = MessageAdapter.toUI(message)

      expect(result.content[0].type).toBe('tool')
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('pending')
      }
    })

    it('应该保留其他状态不变', () => {
      const statuses: Array<'queued' | 'running' | 'done' | 'error'> = ['queued', 'running', 'done', 'error']
      
      statuses.forEach(status => {
        const toolBlock: ToolBlock = {
          id: `tool-${status}`,
          type: 'tool',
          callId: `call-${status}`,
          name: 'test_tool',
          status,
          argumentsText: '{}',
          outputText: '',
          resultText: '',
          isError: status === 'error'
        }
        
        const message: Message = {
          id: `msg-${status}`,
          role: 'assistant',
          createdAt: '2024-01-01T10:00:00Z',
          blocks: [toolBlock]
        }

        const result = MessageAdapter.toUI(message)

        expect(result.content[0].type).toBe('tool')
        if (result.content[0].type === 'tool') {
          const expectedStatus = status === 'queued' ? 'pending' : status === 'done' ? 'success' : status
        expect(result.content[0].status).toBe(expectedStatus)
        }
      })
    })
  })
})
