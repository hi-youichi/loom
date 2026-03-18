/**
 * Loom 协议类型与 type guard 测试
 * 覆盖 message_chunk / thought_chunk 区分及 isMessageChunkEvent / isThoughtChunkEvent
 */

import { describe, it, expect } from 'vitest'
import {
  isMessageChunkEvent,
  isThoughtChunkEvent,
  isToolEvent,
  type LoomStreamEvent,
  type LoomMessageChunkEvent,
  type LoomThoughtChunkEvent,
} from '../../types/protocol/loom'
import { createLoomMessageChunkEvent, createLoomThoughtChunkEvent } from '../utils/testFactory'

describe('Loom protocol - message_chunk vs thought_chunk', () => {
  it('isMessageChunkEvent 对 message_chunk 返回 true', () => {
    const event = createLoomMessageChunkEvent({ content: 'reply' })
    expect(isMessageChunkEvent(event)).toBe(true)
    expect(isThoughtChunkEvent(event)).toBe(false)
    if (isMessageChunkEvent(event)) {
      expect(event.type).toBe('message_chunk')
      expect(event.content).toBe('reply')
    }
  })

  it('isThoughtChunkEvent 对 thought_chunk 返回 true', () => {
    const event = createLoomThoughtChunkEvent({ content: 'reasoning' })
    expect(isThoughtChunkEvent(event)).toBe(true)
    expect(isMessageChunkEvent(event)).toBe(false)
    if (isThoughtChunkEvent(event)) {
      expect(event.type).toBe('thought_chunk')
      expect(event.content).toBe('reasoning')
    }
  })

  it('message_chunk 与 thought_chunk 为不同事件类型', () => {
    const msg: LoomMessageChunkEvent = {
      type: 'message_chunk',
      id: 'think',
      content: 'final',
    }
    const thought: LoomThoughtChunkEvent = {
      type: 'thought_chunk',
      id: 'think',
      content: 'thinking',
    }
    expect(msg.type).not.toBe(thought.type)
    expect(isMessageChunkEvent(msg)).toBe(true)
    expect(isMessageChunkEvent(thought)).toBe(false)
    expect(isThoughtChunkEvent(thought)).toBe(true)
    expect(isThoughtChunkEvent(msg)).toBe(false)
  })

  it('isMessageChunkEvent 对 node_enter 等返回 false', () => {
    const nodeEnter: LoomStreamEvent = { type: 'node_enter', id: 'think' }
    expect(isMessageChunkEvent(nodeEnter)).toBe(false)
    expect(isThoughtChunkEvent(nodeEnter)).toBe(false)
  })

  it('isThoughtChunkEvent 对 usage 等返回 false', () => {
    const usage: LoomStreamEvent = {
      type: 'usage',
      prompt_tokens: 1,
      completion_tokens: 2,
      total_tokens: 3,
    }
    expect(isThoughtChunkEvent(usage)).toBe(false)
    expect(isMessageChunkEvent(usage)).toBe(false)
  })

  it('isToolEvent 对 tool_call 返回 true，对 thought_chunk / message_chunk 返回 false', () => {
    expect(isToolEvent(createLoomThoughtChunkEvent())).toBe(false)
    expect(isToolEvent(createLoomMessageChunkEvent())).toBe(false)
    const toolCall: LoomStreamEvent = {
      type: 'tool_call',
      name: 'bash',
      arguments: {},
    }
    expect(isToolEvent(toolCall)).toBe(true)
  })
})
