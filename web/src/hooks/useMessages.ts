import { useState, useCallback } from 'react'
import type { UIMessageItemProps, UIMessageContent } from '../types/ui/message'

/**
 * useMessages Hook - 管理消息状态和操作
 * 
 * 职责：
 * - 消息的CRUD操作
 * - 流式状态管理
 * - 消息更新和合并
 */
export function useMessages() {
  const [messages, setMessages] = useState<UIMessageItemProps[]>([])
  const [isStreaming, setIsStreaming] = useState(false)

  /**
   * 添加新消息
   */
  const addMessage = useCallback((message: UIMessageItemProps) => {
    setMessages(prev => {
      // 避免重复添加
      if (prev.find(m => m.id === message.id)) {
        return prev
      }
      return [...prev, message]
    })
  }, [])

  /**
   * 更新消息
   */
  const updateMessage = useCallback((id: string, updates: Partial<UIMessageItemProps>) => {
    setMessages(prev => 
      prev.map(msg => msg.id === id ? { ...msg, ...updates } : msg)
    )
  }, [])

  /**
   * 向消息添加内容块
   */
  const addBlockToMessage = useCallback((messageId: string, block: UIMessageContent) => {
    setMessages(prev => 
      prev.map(msg => {
        if (msg.id !== messageId) return msg
        
        // 避免重复添加块
        if ('id' in block && msg.content.find(b => 'id' in b && b.id === block.id)) {
          return msg
        }
        
        return {
          ...msg,
          content: [...msg.content, block]
        }
      })
    )
  }, [])

  /**
   * 更新消息中的内容块
   */
  const updateBlockInMessage = useCallback((
    messageId: string, 
    blockId: string, 
    updates: Partial<UIMessageContent>
  ) => {
    setMessages(prev => 
      prev.map(msg => {
        if (msg.id !== messageId) return msg
        
        return {
          ...msg,
          content: msg.content.map(block => {
            if (!('id' in block) || block.id !== blockId) return block
            return { ...block, ...updates }
          })
        }
      })
    )
  }, [])

  /**
   * 追加文本到文本块
   */
  const appendTextToBlock = useCallback((messageId: string, blockIndex: number, text: string) => {
    setMessages(prev => 
      prev.map(msg => {
        if (msg.id !== messageId) return msg
        
        return {
          ...msg,
          content: msg.content.map((block, index) => {
            if (index !== blockIndex || block.type !== 'text') return block
            return {
              ...block,
              text: block.text + text
            }
          })
        }
      })
    )
  }, [])

  /**
   * 移除消息
   */
  const removeMessage = useCallback((id: string) => {
    setMessages(prev => prev.filter(msg => msg.id !== id))
  }, [])

  /**
   * 清空所有消息
   */
  const clearMessages = useCallback(() => {
    setMessages([])
  }, [])

  /**
   * 获取消息
   */
  const getMessage = useCallback((id: string) => {
    return messages.find(msg => msg.id === id)
  }, [messages])

  return {
    // 状态
    messages,
    isStreaming,
    
    // 操作
    addMessage,
    updateMessage,
    addBlockToMessage,
    updateBlockInMessage,
    appendTextToBlock,
    removeMessage,
    clearMessages,
    getMessage,
    setIsStreaming,
  }
}

export type UseMessagesReturn = ReturnType<typeof useMessages>
