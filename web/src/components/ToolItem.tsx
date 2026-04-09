import { useState } from 'react'
import type { ToolBlock, ToolType } from '../types/chat'
import { TOOL_TYPE_INFO, TOOL_STATUS_INFO } from '../types/toolConfig'

interface ToolItemProps {
  tool: ToolBlock
  defaultExpanded?: boolean
  onAction?: (action: string, tool: ToolBlock) => void
}

export function ToolItem({ tool, defaultExpanded = false, onAction }: ToolItemProps) {
  const [expanded, setExpanded] = useState(defaultExpanded)
  const [showFullOutput, setShowFullOutput] = useState(false)

  // 自动检测工具类型
  const toolType = tool.toolType || detectToolType(tool.name)
  const typeInfo = TOOL_TYPE_INFO[toolType]
  const statusInfo = TOOL_STATUS_INFO[tool.status]

  const { truncated, hasMore } = truncateOutput(tool.outputText || '', 8)
  const displayOutput = showFullOutput ? (tool.outputText || '') : truncated
  const hasOutput = (tool.outputText?.length || 0) > 0
  const hasArgs = (tool.argumentsText?.length || 0) > 0

  return (
    <article
      className={`tool-item tool-item--${toolType} tool-item--${tool.status}`}
      style={{ borderLeftColor: typeInfo.color }}
      aria-label={`${typeInfo.label}工具: ${tool.name}`}
    >
      <button
        className="tool-item__header"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        type="button"
      >
        <span className="tool-item__icon" aria-hidden="true">
          {statusInfo.icon}
        </span>
        <span className="tool-item__type-icon" style={{ color: typeInfo.color }}>
          {typeInfo.icon}
        </span>
        <span className="tool-item__name">{tool.name}</span>
        <span className="tool-item__type-label" style={{ color: typeInfo.color }}>
          {typeInfo.label}
        </span>
        <span className="tool-item__status" style={{ color: statusInfo.color }}>
          {statusInfo.label}
        </span>
        <span className="tool-item__toggle" aria-hidden="true">
          {expanded ? '▼' : '▶'}
        </span>
      </button>

      {expanded && (
        <div className="tool-item__content">
          {/* 工具元信息 */}
          <div className="tool-item__meta">
            <span>类型: {typeInfo.description}</span>
            {tool.timestamp && <span>时间: {new Date(tool.timestamp).toLocaleTimeString()}</span>}
            {tool.duration && <span>耗时: {tool.duration}ms</span>}
          </div>

          {/* 参数显示 */}
          {hasArgs && (
            <div className="tool-item__section">
              <div className="tool-item__label">参数</div>
              <pre className="tool-item__args">{formatArguments(tool.argumentsText)}</pre>
            </div>
          )}

          {/* 输出显示 */}
          {hasOutput && (
            <div className="tool-item__section">
              <div className="tool-item__label">输出</div>
              <pre className="tool-item__output">{displayOutput}</pre>
              {hasMore && !showFullOutput && (
                <button
                  className="tool-item__more"
                  onClick={() => setShowFullOutput(true)}
                  type="button"
                >
                  展开更多 ({tool.outputText.split('\n').length} 行)
                </button>
              )}
              {showFullOutput && hasMore && (
                <button
                  className="tool-item__more"
                  onClick={() => setShowFullOutput(false)}
                  type="button"
                >
                  收起
                </button>
              )}
            </div>
          )}

          {/* 结果显示 */}
          {tool.resultText && (
            <div className="tool-item__section">
              <div className="tool-item__label">结果</div>
              <pre className="tool-item__result">{tool.resultText}</pre>
            </div>
          )}

          {/* 错误显示 */}
          {tool.isError && tool.resultText && (
            <div className="tool-item__section tool-item__section--error">
              <div className="tool-item__label">错误信息</div>
              <pre className="tool-item__error-text">{tool.resultText}</pre>
            </div>
          )}

          {/* 操作按钮 */}
          {onAction && (
            <div className="tool-item__actions">
              <button onClick={() => onAction('retry', tool)} type="button">
                重试
              </button>
              <button onClick={() => onAction('copy', tool)} type="button">
                复制结果
              </button>
              {tool.status === 'approval_required' && (
                <button onClick={() => onAction('approve', tool)} type="button">
                  批准
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </article>
  )
}

// 工具类型检测函数
function detectToolType(name: string): ToolType {
  const lowerName = name.toLowerCase()
  
  if (lowerName.includes('read') || lowerName.includes('get') || lowerName.includes('load')) {
    return 'read'
  }
  if (lowerName.includes('edit') || lowerName.includes('update') || lowerName.includes('modify')) {
    return 'edit'
  }
  if (lowerName.includes('delete') || lowerName.includes('remove') || lowerName.includes('rm')) {
    return 'delete'
  }
  if (lowerName.includes('move') || lowerName.includes('rename') || lowerName.includes('mv')) {
    return 'move'
  }
  if (lowerName.includes('search') || lowerName.includes('find') || lowerName.includes('grep')) {
    return 'search'
  }
  if (lowerName.includes('execute') || lowerName.includes('run') || lowerName.includes('exec')) {
    return 'execute'
  }
  if (lowerName.includes('think') || lowerName.includes('reason') || lowerName.includes('plan')) {
    return 'think'
  }
  if (lowerName.includes('fetch') || lowerName.includes('request') || lowerName.includes('http')) {
    return 'fetch'
  }
  
  return 'other'
}

// 辅助函数
function truncateOutput(text: string, maxLines: number): { truncated: string; hasMore: boolean } {
  if (!text) {
    return { truncated: '', hasMore: false }
  }
  const lines = text.split('\n')
  if (lines.length <= maxLines) {
    return { truncated: text, hasMore: false }
  }

  const truncated = lines.slice(0, maxLines).join('\n')
  return { truncated, hasMore: true }
}

function formatArguments(argsText: string | undefined | null): string {
  if (!argsText) {
    return ''
  }
  try {
    const parsed = JSON.parse(argsText)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return argsText
  }
}