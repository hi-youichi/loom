import { memo } from 'react'
import type { UIToolContent } from '../../types/ui/message'
import { TOOL_TYPE_INFO, TOOL_STATUS_INFO } from '../../types/toolConfig'

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
  // 使用工具类型信息
  const toolType = content.toolType || 'other'
  const typeInfo = TOOL_TYPE_INFO[toolType]
  const statusInfo = TOOL_STATUS_INFO[content.status]

  return (
    <div 
      className={`tool-message tool-message--${toolType} ${className || ''}`} 
      role="article" 
      aria-label={`${typeInfo.label}工具: ${content.name}`}
      style={{ borderLeftColor: typeInfo.color }}
    >
      <div className="tool-message__header">
        <span className="tool-message__icon">{statusInfo.icon}</span>
        <span className="tool-message__type-icon" style={{ color: typeInfo.color }}>
          {typeInfo.icon}
        </span>
        <span className="tool-message__name">{content.name}</span>
        <span className="tool-message__type-label" style={{ color: typeInfo.color }}>
          {typeInfo.label}
        </span>
        <span 
          className="tool-message__status"
          style={{ color: statusInfo.color }}
          aria-label={`状态: ${content.status}`}
        >
          {statusInfo.label}
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
