/**
 * 消息适配器
 * 将 Loom 协议事件转换为通用 UI 消息类型
 */

import type { LoomStreamEvent } from '../types/protocol/loom'
import type { UIMessageItemProps, UITextContent, UIToolContent } from '../types/ui/message'

/**
 * 消息适配器类
 * 负责将协议数据转换为UI组件可用的数据
 */
export class MessageAdapter {
  /**
   * 将单个 Loom 事件转换为 UI 消息
   */
  static toUI(event: LoomStreamEvent): UIMessageItemProps {
    const base = {
      id: event.id,
      timestamp: event.createdAt,
      sender: event.type === 'user' ? 'user' as const : 'assistant' as const,
    }

    if (event.type === 'user') {
      return {
        ...base,
        content: [this.createTextContent(event.text)]
      }
    }

    if (event.type === 'assistant_text') {
      return {
        ...base,
        content: [this.createTextContent(event.text)]
      }
    }

    if (event.type === 'assistant_tool') {
      return {
        ...base,
        content: [this.createToolContent(event)]
      }
    }

    // 兜底处理（不应该到达这里）
    return {
      ...base,
      content: [this.createTextContent('Unknown event type')]
    }
  }

  /**
   * 将多个 Loom 事件转换为 UI 消息列表
   */
  static toUIList(events: LoomStreamEvent[]): UIMessageItemProps[] {
    return events.map(event => this.toUI(event))
  }

  /**
   * 创建文本内容
   */
  private static createTextContent(text: string): UITextContent {
    return {
      type: 'text',
      text,
      format: 'plain'
    }
  }

  /**
   * 创建工具内容
   */
path(event: Extract<LoomStreamEvent, { type: 'assistant_tool' }>): UIToolContent {
    return {
      type: 'tool',
      id: event.callId,
      name: event.name,
      status: this.mapToolStatus(event.status),
      argumentsText: event.argumentsText,
      outputText: event.outputText,
      resultText: event.resultText,
      isError: event.isError,
    }
  }

  /**
   * 映射工具状态
   */
  private static mapToolStatus(
    loomStatus: 'queued' | 'running' | 'done' | 'error' | 'approval_required'
  ): 'pending' | 'running' | 'success' | 'error' {
    switch (loomStatus) {
      case 'queued':
      case 'approval_required':
        return 'pending'
      case 'running':
        return 'running'
      case 'done':
        return 'success'
      case 'error':
        return 'error'
      default:
        return 'pending'
    }
  }

  /**
   * 合并多个事件为一个消息（用于助手响应）
   * 例如：一个助手消息可能包含多个文本块和工具调用
   */
  static mergeEvents(events: LoomStreamEvent[]): UIMessageItemProps[] {
    const messages: UIMessageItemProps[] = []
    let currentMessage: UIMessageItemProps | null = null

    for (const event of events) {
      if (event.type === 'user') {
        // 用户消息总是独立的
        if (currentMessage) {
          messages.push(currentMessage)
        }
        currentMessage = this.toUI(event)
        messages.push(currentMessage)
        currentMessage = null
      } else if (event.type === 'assistant_text' || event.type === 'assistant_tool') {
        // 助手消息可能需要合并
        if (!currentMessage) {
          currentMessage = {
            id: event.id,
            sender: 'assistant',
            timestamp: event.createdAt,
            content: []
          }
        }
        
        // 添加内容到当前消息
        if (event.type === 'assistant_text') {
          currentMessage.content.push(this.createTextContent(event.text))
        } else {
          currentMessage.content.push(this.createToolContent(event))
        }
      }
    }

    // 添加最后一个消息
    if (currentMessage) {
      messages.push(currentMessage)
    }

    return messages
  }

  /**
   * 更新现有消息（用于流式更新）
   */
  static updateMessage(
    message: UIMessageItemProps,
    event: LoomStreamEvent
  ): UIMessageItemProps {
    if (event.type === 'assistant_text') {
      // 更新或添加文本内容
      const lastContent = message.content[message.content.length - 1]
      if (lastContent && lastContent.type === 'text') {
        // 追加文本
        return {
          ...message,
          content: [
            ...message.content.slice(0, -1),
            { ...lastContent, text: lastContent.text + event.text }
          ]
        }
      } else {
        // 添加新的文本块
        return {
          ...message,
          content: [...message.content, this.createTextContent(event.text)]
        }
      }
    }

    if (event.type === 'assistant_tool') {
      // 添加或更新工具块
      const existingToolIndex = message.content.findIndex(
        c => c.type === 'tool' && c.id === event.callId
      )

      if (existingToolIndex >= 0) {
        // 更新现有工具块
        const newContent = [...message.content]
        newContent[existingToolIndex] = this.createToolContent(event)
        return { ...message, content: newContent }
      } else {
        // 添加新工具块
        return {
          ...message,
          content: [...message.content, this.createToolContent(event)]
        }
      }
    }

    // 其他情况返回原消息
    return message
  }
}
