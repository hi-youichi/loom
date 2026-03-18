/**
 * Loom 协议类型定义
 * 这些类型直接对应后端 Loom 协议的数据结构
 */

// ============= WebSocket 消息类型 =============

/**
 * WebSocket 连接状态
 */
export type WebSocketStatus = 'connecting' | 'connected' | 'disconnected' | 'error'

/**
 * WebSocket 消息基类
 */
export interface WebSocketMessage {
  type: string
  id?: string
  timestamp?: string
}

// ============= Loom 流事件类型 =============

/**
 * 用户事件
 */
export interface LoomUserEvent {
  type: 'user'
  id: string
  createdAt: string
  text: string
}

/**
 * 助手文本事件
 */
export interface LoomAssistantTextEvent {
  type: 'assistant_text'
  id: string
  createdAt: string
  text: string
}

/**
 * 工具状态
 */
export type LoomToolStatus = 'queued' | 'running' | 'done' | 'error' | 'approval_required'

/**
 * 助手工具事件
 */
export interface LoomAssistantToolEvent {
  type: 'assistant_tool'
  id: string
  createdAt: string
  callId: string
  name: string
  status: LoomToolStatus
  argumentsText: string
  outputText: string
  resultText: string
  isError: boolean
}

/**
 * 所有 Loom 流事件的联合类型
 */
export type LoomStreamEvent = LoomUserEvent | LoomAssistantTextEvent | LoomAssistantToolEvent

// ============= WebSocket 响应类型 =============

/**
 * 流事件响应
 */
export interface LoomRunStreamEventResponse {
  type: 'run_stream_event'
  id: string
  event: LoomStreamEvent
}

/**
 * 运行结束响应
 */
export interface LoomRunEndResponse {
  type: 'run_end'
  id: string
  reply: string
}

/**
 * 错误响应
 */
export interface LoomErrorResponse {
  type: 'error'
  id?: string
  error: string
}

/**
 * 所有 Loom 服务器消息的联合类型
 */
export type LoomServerMessage =
  | LoomRunStreamEventResponse
  | LoomRunEndResponse
  | LoomErrorResponse
  | { type: string } // 兜底类型

// ============= 类型守卫 =============

/**
 * 检查是否为用户事件
 */
export function isUserEvent(event: LoomStreamEvent): event is LoomUserEvent {
  return event.type === 'user'
}

/**
 * 检查是否为助手文本事件
 */
export function isAssistantTextEvent(event: LoomStreamEvent): event is LoomAssistantTextEvent {
  return event.type === 'assistant_text'
}

/**
 * 检查是否为助手工具事件
 */
export function isAssistantToolEvent(event: LoomStreamEvent): event is LoomAssistantToolEvent {
  return event.type === 'assistant_tool'
}

/**
 * 检查是否为流事件响应
 */
export function isRunStreamEvent(msg: LoomServerMessage): msg is LoomRunStreamEventResponse {
  return msg.type === 'run_stream_event'
}

/**
 * 检查是否为运行结束响应
 */
export function isRunEnd(msg: LoomServerMessage): msg is LoomRunEndResponse {
  return msg.type === 'run_end'
}

/**
 * 检查是否为错误响应
 */
export function isError(msg: LoomServerMessage): msg is LoomErrorResponse {
  return msg.type === 'error'
}

// ============= 发送消息类型 =============

/**
 * 发送聊天消息的请求
 */
export interface SendMessageRequest {
  type: 'send_message'
  threadId: string
  text: string
}

/**
 * 聊天响应
 */
export interface ChatReply {
  content: string
}
