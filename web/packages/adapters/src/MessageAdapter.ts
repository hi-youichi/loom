/**
 * 消息适配器
 * 将规范化后的聊天消息转换为通用 UI 消息类型
 */

import type { UIMessageItemProps, UITextContent, UIToolContent } from '@loom/types'
import type { Message, MessageBlock, ToolBlock } from '@loom/types'
import { ToolBlockAdapter } from './ToolBlockAdapter'

/**
 * 消息适配器类
 * 负责将协议数据转换为UI组件可用的数据
 */
export class MessageAdapter {
  /**
   * 将单条规范化消息转换为 UI 消息
   */
  static toUI(message: Message): UIMessageItemProps {
    return {
      id: message.id,
      timestamp: message.createdAt,
      sender: message.role,
      content: message.blocks.map((block) => this.createContent(block)),
    }
  }

  /**
   * 将多条规范化消息转换为 UI 消息列表
   */
  static toUIList(messages: Message[]): UIMessageItemProps[] {
    return messages.map((message) => this.toUI(message))
  }

  private static createTextContent(text: string): UITextContent {
    return {
      type: 'text',
      text,
      format: 'plain',
    }
  }

  private static createContent(block: MessageBlock): UITextContent | UIToolContent {
    if (block.type === 'text') {
      return this.createTextContent(block.text)
    }

    return this.createToolContent(block)
  }

  private static createToolContent(block: ToolBlock): UIToolContent {
    return ToolBlockAdapter.toUI({
      callId: block.callId,
      name: block.name,
      status: block.status === 'approval_required' ? 'queued' : block.status,
      argumentsText: block.argumentsText,
      outputText: block.outputText,
      resultText: block.resultText,
      isError: block.isError,
    })
  }
}
