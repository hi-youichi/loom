/**
 * 历史消息解析器
 * 将 user_messages API 返回的扁平 (role, content) 列表
 * 重建为包含工具块的 UIMessageItemProps[]
 *
 * 后端数据格式:
 * - user:    { role: "user",    content: "纯文本" }
 * - assistant (纯文本): { role: "assistant", content: "纯文本" }
 * - assistant (带工具):  { role: "assistant", content: '{"content":"...","tool_calls":[...]}' }
 * - tool:    { role: "tool",    content: '{"tool_call_id":"...","content":"..."}' }
 */

import type { UIMessageItemProps, UITextContent, UIToolContent } from '../types/ui/message'
import type { UserMessageItem } from '../services/userMessages'

// ── 内部类型 ──────────────────────────────────────────

interface AssistantToolCall {
  id: string
  name: string
  arguments: string
}

interface AssistantPayload {
  content: string
  tool_calls?: AssistantToolCall[]
  reasoning_content?: string
}

interface ToolResultPayload {
  tool_call_id: string
  content: string
}

// ── 辅助函数 ──────────────────────────────────────────

function tryParseJSON(text: string): unknown | null {
  try {
    return JSON.parse(text)
  } catch {
    return null
  }
}

/** 尝试将 content 解析为 AssistantPayload */
function tryParseAssistantPayload(content: string): AssistantPayload | null {
  const trimmed = content.trimStart()
  if (!trimmed.startsWith('{')) return null
  const parsed = tryParseJSON(content)
  if (!parsed || typeof parsed !== 'object') return null
  // 必须包含 content 字段才认为是 AssistantPayload
  if (!('content' in parsed)) return null
  return parsed as AssistantPayload
}

/** 尝试将 content 解析为 ToolResultPayload */
function tryParseToolResult(content: string): ToolResultPayload | null {
  const parsed = tryParseJSON(content)
  if (!parsed || typeof parsed !== 'object') return null
  if (!('tool_call_id' in parsed) || !('content' in parsed)) return null
  return parsed as ToolResultPayload
}

function createTextContent(text: string): UITextContent {
  return { type: 'text', text, format: 'plain' }
}

function createToolContent(
  callId: string,
  name: string,
  status: 'pending' | 'running' | 'success' | 'error',
  argumentsText: string,
  outputText: string,
  resultText: string,
  isError: boolean,
): UIToolContent {
  return {
    type: 'tool',
    id: callId,
    name,
    status,
    argumentsText,
    outputText,
    resultText,
    isError,
  }
}

/** 格式化 arguments 用于显示 */
function formatArguments(args: unknown): string {
  if (typeof args === 'string') {
    // 尝试 pretty-print JSON
    try {
      return JSON.stringify(JSON.parse(args), null, 2)
    } catch {
      return args
    }
  }
  try {
    return JSON.stringify(args, null, 2)
  } catch {
    return String(args)
  }
}

// ── 主解析逻辑 ──────────────────────────────────────────

/**
 * 将扁平的历史消息列表重建为 UI 消息列表
 *
 * 策略:
 * 1. user 消息 → 独立的 user UIMessageItemProps
 * 2. assistant 消息 → 收集其后的 tool 消息，合并为一个 assistant UIMessageItemProps
 *    - 纯文本 assistant → 文本块
 *    - 带 tool_calls 的 assistant → 文本块 + 工具块（附带 tool 结果）
 * 3. tool 消息 → 不生成独立消息，附加到前一个 assistant 消息的工具块中
 */
export function parseHistoryMessages(items: UserMessageItem[]): UIMessageItemProps[] {
  const result: UIMessageItemProps[] = []

  // 先按顺序遍历，建立 tool_call_id → ToolResultPayload 的映射
  const toolResults = new Map<string, ToolResultPayload>()
  for (const item of items) {
    if (item.role === 'tool') {
      const parsed = tryParseToolResult(item.content)
      if (parsed && parsed.tool_call_id) {
        toolResults.set(parsed.tool_call_id, parsed)
      }
    }
  }

  // 按顺序重建消息
  for (const item of items) {
    // 跳过 system 消息（不在聊天 UI 中显示）
    if (item.role === 'system') continue

    // user 消息直接创建
    if (item.role === 'user') {
      result.push({
        id: crypto.randomUUID(),
        sender: 'user',
        timestamp: new Date().toISOString(),
        content: [createTextContent(item.content)],
      })
      continue
    }

    // tool 消息不创建独立 UI 消息，它是前一个 assistant 消息的附属
    if (item.role === 'tool') continue

    // assistant 消息
    if (item.role === 'assistant') {
      const payload = tryParseAssistantPayload(item.content)

      if (!payload) {
        // 纯文本 assistant 消息
        result.push({
          id: crypto.randomUUID(),
          sender: 'assistant',
          timestamp: new Date().toISOString(),
          content: [createTextContent(item.content)],
        })
        continue
      }

      // 带 tool_calls 的 assistant 消息
      const contentBlocks: (UITextContent | UIToolContent)[] = []

      // 添加文本块（如果有内容）
      const textContent = payload.content?.trim()
      if (textContent) {
        contentBlocks.push(createTextContent(textContent))
      }

      // 添加工具块
      if (payload.tool_calls && payload.tool_calls.length > 0) {
        for (const tc of payload.tool_calls) {
          const callId = tc.id
          const toolResult = toolResults.get(callId)
          const argsText = formatArguments(tc.arguments)
          // 历史工具结果：有 tool_result 则视为成功
          const outputText = toolResult?.content ?? ''

          contentBlocks.push(
            createToolContent(
              callId,
              tc.name,
              toolResult ? 'success' : 'success',
              argsText,
              outputText,
              '', // resultText 在历史中与 outputText 含义相同
              false,
            ),
          )
        }
      }

      // 如果完全没有内容（极少见），至少显示一个空文本块
      if (contentBlocks.length === 0) {
        contentBlocks.push(createTextContent(''))
      }

      result.push({
        id: crypto.randomUUID(),
        sender: 'assistant',
        timestamp: new Date().toISOString(),
        content: contentBlocks,
      })
      continue
    }
  }

  return result
}
