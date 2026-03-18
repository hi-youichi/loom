/**
 * 通用 UI 消息类型
 * 这些类型与协议无关，可以被任何聊天组件使用
 */

/** 消息发送者 */
export type UIMessageSender = 'user' | 'assistant' | 'system'

/** 文本内容 */
export interface UITextContent {
  type: 'text'
  text: string
  format?: 'plain' | 'markdown' | 'html'
}

/** 工具状态 */
export type UIToolStatus = 'pending' | 'running' | 'success' | 'error'

/** 工具内容 */
export interface UIToolContent {
  type: 'tool'
  id: string
  name: string
  status: UIToolStatus
  argumentsText: string
  outputText: string
  resultText: string
  isError: boolean
}

/** 消息内容联合类型 */
export type UIMessageContent = UITextContent | UIToolContent

/** 消息项属性 - 组件使用的主要类型 */
export interface UIMessageItemProps {
  id: string
  sender: UIMessageSender
  timestamp: string
  content: UIMessageContent[]
  className?: string
  onRetry?: () => void
}

/** 消息列表属性 */
export interface UIMessageListProps {
  messages: UIMessageItemProps[]
  isLoading?: boolean
  className?: string
  onMessageClick?: (messageId: string) => void
}

/** 类型守卫 */
export function isUITextContent(content: UIMessageContent): content is UITextContent {
  return content.type === 'text'
}

export function isUIToolContent(content: UIMessageContent): content is UIToolContent {
  return content.type === 'tool'
}
