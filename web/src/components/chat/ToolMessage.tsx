import { memo } from 'react'
import type { UIToolContent } from '../../types/ui/message'

type ToolMessageProps = {
  content: UIToolContent
  className?: string
}

/**
 * ToolMessage组件 - 协议无关的工具消息显示
 * 支持多种工具状态的可视化
 */
export const ToolMessage = memo(function ToolMessage({ 
  content,
  className 
}: ToolMessageProps) {
  const statusColor = {
    pending: '#999',
    running: '#2196F3',
    success: '#4CAF50',
    error: '#F44336',
  }

  const statusText = {
    pending: '⏳ 等待中',
    running: '🔄 执行中',
    success: '✅ 成功',
    error: '❌ 失败',
  }

  return (
    <div 
      className={`tool-message ${className || ''}`} 
      role="article" 
      aria-label={`工具调用: ${content.name}`}
    >
      <div className="tool-message__header">
        <span className="tool-message__name">{content.name}</span>
        <span 
          className="tool-message__status"
          style={{ color: statusColor[content.status] }}
          aria-label={`状态: ${content.status}`}
        >
          {statusText[content.status]}
        </span>
      </div>
      
      {content.argumentsText && (
        <details className="tool-message__arguments">
          <summary>参数</summary>
          <pre>{content.argumentsText}</pre>
        </details>
      )}
      
      {content.outputText && (
        <details className="tool-message__output">
          <summary>输出</summary>
          <pre>{content.outputText}</pre>
        </details>
      )}
      
      {content.resultText && (
        <details className="tool-message__result">
          <summary>结果</summary>
          <pre>{content.resultText}</pre>
        </details>
      )}
      
      {content.isError && (
        <div className="tool-message__error" role="alert">
          <strong>错误:</strong> 工具执行失败
        </div>
      )}
    </div>
  )
})
