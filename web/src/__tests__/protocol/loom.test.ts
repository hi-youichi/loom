import { describe, it, expect } from 'vitest'
import {
  isMessageChunkEvent,
  isToolEvent,
  isRunStreamEvent,
  isRunEnd,
  isError,
  isWorkspaceResponse,
  type LoomStreamEvent,
  type LoomMessageChunkEvent,
  type LoomThoughtChunkEvent,
  type LoomRunStreamEventResponse,
  type LoomRunEndResponse,
  type LoomErrorResponse,
} from '../../types/protocol/loom'

describe('Loom protocol - type guards', () => {
  it('isMessageChunkEvent returns true for message_chunk', () => {
    const event: LoomMessageChunkEvent = { type: 'message_chunk', id: 'think', content: 'reply' }
    expect(isMessageChunkEvent(event)).toBe(true)
    if (isMessageChunkEvent(event)) {
      expect(event.type).toBe('message_chunk')
      expect(event.content).toBe('reply')
    }
  })

  it('isMessageChunkEvent returns false for thought_chunk', () => {
    const event: LoomThoughtChunkEvent = { type: 'thought_chunk', id: 'think', content: 'reasoning' }
    expect(isMessageChunkEvent(event)).toBe(false)
  })

  it('isMessageChunkEvent returns false for node_enter', () => {
    const nodeEnter: LoomStreamEvent = { type: 'node_enter', id: 'think' }
    expect(isMessageChunkEvent(nodeEnter)).toBe(false)
  })

  it('isMessageChunkEvent returns false for usage', () => {
    const usage: LoomStreamEvent = {
      type: 'usage',
      prompt_tokens: 1,
      completion_tokens: 2,
      total_tokens: 3,
    }
    expect(isMessageChunkEvent(usage)).toBe(false)
  })

  it('message_chunk and thought_chunk are different types', () => {
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
  })

  it('isToolEvent returns true for tool_call', () => {
    const toolCall: LoomStreamEvent = {
      type: 'tool_call',
      name: 'bash',
      arguments: {},
    }
    expect(isToolEvent(toolCall)).toBe(true)
  })

  it('isToolEvent returns true for tool_start', () => {
    const event: LoomStreamEvent = { type: 'tool_start', call_id: 'c1', name: 'read' }
    expect(isToolEvent(event)).toBe(true)
  })

  it('isToolEvent returns true for tool_output', () => {
    const event: LoomStreamEvent = { type: 'tool_output', call_id: 'c1', output: 'data' }
    expect(isToolEvent(event)).toBe(true)
  })

  it('isToolEvent returns true for tool_end', () => {
    const event: LoomStreamEvent = { type: 'tool_end', call_id: 'c1' }
    expect(isToolEvent(event)).toBe(true)
  })

  it('isToolEvent returns true for tool_call_chunk', () => {
    const event: LoomStreamEvent = { type: 'tool_call_chunk', call_id: 'c1', name: 'read', arguments_chunk: '{}' }
    expect(isToolEvent(event)).toBe(true)
  })

  it('isToolEvent returns false for thought_chunk and message_chunk', () => {
    const thought: LoomStreamEvent = { type: 'thought_chunk', id: 't1', content: 'thinking' }
    const msg: LoomStreamEvent = { type: 'message_chunk', id: 'm1', content: 'hello' }
    expect(isToolEvent(thought)).toBe(false)
    expect(isToolEvent(msg)).toBe(false)
  })

  it('isRunStreamEvent returns true for run_stream_event', () => {
    const msg: LoomRunStreamEventResponse = {
      type: 'run_stream_event',
      id: 'run-1',
      event: { type: 'message_chunk', id: 'm1', content: 'hi' },
    }
    expect(isRunStreamEvent(msg)).toBe(true)
  })

  it('isRunEnd returns true for run_end', () => {
    const msg: LoomRunEndResponse = { type: 'run_end', id: 'run-1', reply: 'Done' }
    expect(isRunEnd(msg)).toBe(true)
  })

  it('isError returns true for error', () => {
    const msg: LoomErrorResponse = { type: 'error', id: 'e1', error: 'fail' }
    expect(isError(msg)).toBe(true)
  })

  it('isError returns false for non-error', () => {
    const msg = { type: 'run_end', id: 'r1', reply: 'ok' }
    expect(isError(msg as any)).toBe(false)
  })

  it('isWorkspaceResponse returns true for workspace_list', () => {
    const msg = { type: 'workspace_list', id: 'w1', workspaces: [] }
    expect(isWorkspaceResponse(msg as any)).toBe(true)
  })

  it('isWorkspaceResponse returns true for workspace_create', () => {
    const msg = { type: 'workspace_create', id: 'w2', workspace: { id: 'ws-1', created_at_ms: 0 } }
    expect(isWorkspaceResponse(msg as any)).toBe(true)
  })

  it('isWorkspaceResponse returns true for workspace_thread_list', () => {
    const msg = { type: 'workspace_thread_list', id: 'w3', workspace_id: 'ws-1', threads: [] }
    expect(isWorkspaceResponse(msg as any)).toBe(true)
  })

  it('isWorkspaceResponse returns true for workspace_thread_add', () => {
    const msg = { type: 'workspace_thread_add', id: 'w4', workspace_id: 'ws-1', thread_id: 't1' }
    expect(isWorkspaceResponse(msg as any)).toBe(true)
  })

  it('isWorkspaceResponse returns true for workspace_thread_remove', () => {
    const msg = { type: 'workspace_thread_remove', id: 'w5', workspace_id: 'ws-1', thread_id: 't1' }
    expect(isWorkspaceResponse(msg as any)).toBe(true)
  })

  it('isWorkspaceResponse returns false for run_stream_event', () => {
    const msg = { type: 'run_stream_event', id: 'r1', event: {} }
    expect(isWorkspaceResponse(msg as any)).toBe(false)
  })
})
