import { useState } from 'react'
import type { ToolBlock } from '../types/chat'

type ToolBlockViewProps = {
  tool: ToolBlock
  defaultExpanded?: boolean
}

const STATUS_ICONS: Record<ToolBlock['status'], string> = {
  queued: '⏳',
  running: '▶',
  done: '✅',
  error: '❌',
  approval_required: '🔒',
}

const STATUS_LABELS: Record<ToolBlock['status'], string> = {
  queued: '等待中',
  running: '运行中',
  done: '已完成',
  error: '错误',
  approval_required: '需审批',
}

const MAX_OUTPUT_LINES = 8

function truncateOutput(text: string | undefined | null, maxLines: number): { truncated: string; hasMore: boolean } {
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

export function ToolBlockView({ tool, defaultExpanded = false }: ToolBlockViewProps) {
  const [expanded, setExpanded] = useState(defaultExpanded)
  const [showFullOutput, setShowFullOutput] = useState(false)

  const { truncated, hasMore } = truncateOutput(tool.outputText, MAX_OUTPUT_LINES)
  const displayOutput = showFullOutput ? tool.outputText : truncated
  const hasOutput = tool.outputText.length > 0
  const hasArgs = tool.argumentsText.length > 0

  return (
    <article
      className={`tool-block tool-block--${tool.status}${tool.isError ? ' tool-block--error' : ''}`}
      aria-label={`Tool: ${tool.name}`}
    >
      <button
        className="tool-block__header"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        type="button"
      >
        <span className="tool-block__icon" aria-hidden="true">
          {STATUS_ICONS[tool.status]}
        </span>
        <span className="tool-block__name">{tool.name}</span>
        <span className="tool-block__status">{STATUS_LABELS[tool.status]}</span>
        <span className="tool-block__toggle" aria-hidden="true">
          {expanded ? '▼' : '▶'}
        </span>
      </button>

      {expanded && (
        <div className="tool-block__content">
          {hasArgs && (
            <div className="tool-block__section">
              <div className="tool-block__label">参数</div>
              <pre className="tool-block__args">{formatArguments(tool.argumentsText)}</pre>
            </div>
          )}

          {hasOutput && (
            <div className="tool-block__section">
              <div className="tool-block__label">输出</div>
              <pre className="tool-block__output">{displayOutput}</pre>
              {hasMore && !showFullOutput && (
                <button
                  className="tool-block__more"
                  onClick={() => setShowFullOutput(true)}
                  type="button"
                >
                  展开更多 ({tool.outputText.split('\n').length} 行)
                </button>
              )}
              {showFullOutput && hasMore && (
                <button
                  className="tool-block__more"
                  onClick={() => setShowFullOutput(false)}
                  type="button"
                >
                  收起
                </button>
              )}
            </div>
          )}

          {tool.resultText && (
            <div className="tool-block__section">
              <div className="tool-block__label">结果</div>
              <pre className="tool-block__result">{tool.resultText}</pre>
            </div>
          )}

          {tool.isError && tool.resultText && (
            <div className="tool-block__section tool-block__section--error">
              <div className="tool-block__label">错误信息</div>
              <pre className="tool-block__error-text">{tool.resultText}</pre>
            </div>
          )}
        </div>
      )}
    </article>
  )
}
