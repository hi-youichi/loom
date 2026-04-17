import { useState, useRef, useEffect } from 'react'
import type { ToolBlock, ToolType } from '@loom/types'
import { TOOL_TYPE_INFO } from '@loom/types'
import { ToolIcon } from './ToolIcon'
import { extractToolTitle, getToolDisplayName } from '@loom/utils'

interface ToolCardProps {
  tool: ToolBlock
  defaultExpanded?: boolean
  onAction?: (action: string, tool: ToolBlock) => void
}

export function ToolCard({ 
  tool, 
  defaultExpanded = false, 
  onAction,
}: ToolCardProps) {
  const [expanded, setExpanded] = useState(defaultExpanded)
  const [showFullOutput, setShowFullOutput] = useState(false)
  const contentRef = useRef<HTMLDivElement>(null)

  // 自动检测工具类型
  const toolType = tool.toolType || detectToolType(tool.name)
  const typeInfo = TOOL_TYPE_INFO[toolType]

  let parsedArgs: Record<string, unknown> = {}
  try { parsedArgs = JSON.parse(tool.argumentsText || '{}') } catch {}
  const displayName = getToolDisplayName(tool.name)
  const displayTitle = extractToolTitle(tool.name, parsedArgs)
  const headerText = displayTitle ? `${displayName} · ${displayTitle}` : displayName

  const { truncated, hasMore } = truncateOutput(tool.outputText || '', 6)
  const displayOutput = showFullOutput ? (tool.outputText || '') : truncated
  const hasOutput = (tool.outputText?.length || 0) > 0
  const hasArgs = (tool.argumentsText?.length || 0) > 0

  // 键盘导航支持
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Enter' || e.key === ' ') {
        if (e.target === contentRef.current) {
          e.preventDefault()
          setExpanded(!expanded)
        }
      }
    }
    
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [expanded])

  return (
    <div 
      className={`tool-card tool-card--${toolType} tool-card--${tool.status}${expanded ? ' tool-card--expanded' : ''}`}
      role="article"
      aria-label={`${displayName} ${displayTitle || ''}`}
    >
      {/* 标题栏 */}
      <div
        ref={contentRef}
        className="tool-card__header"
        onClick={() => setExpanded(!expanded)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            setExpanded(!expanded)
          }
        }}
        tabIndex={0}
        role="button"
        aria-expanded={expanded}
        aria-controls={`tool-content-${tool.id}`}
      >
        <ToolIcon name={typeInfo.icon} size={16} style={{ flexShrink: 0 }} />
        <span className="tool-card__name">{headerText}</span>
        <span className={`tool-card__toggle ${expanded ? 'tool-card__toggle--expanded' : ''}`}>
          ▼
        </span>
      </div>

      {/* 详细内容区域 */}
      {expanded && (
        <div 
          className="tool-card__content" 
          id={`tool-content-${tool.id}`}
          role="region"
          aria-label={`${tool.name} 详细信息`}
        >
          {/* 参数显示 */}
          {hasArgs && (
            <div className="tool-card__section">
              <div className="tool-card__label">输入参数</div>
              <pre className="tool-card__args">{formatArguments(tool.argumentsText)}</pre>
            </div>
          )}

          {/* 输出显示 */}
          {hasOutput && (
            <div className="tool-card__section">
              <div className="tool-card__label">执行输出</div>
              <div className="tool-card__output-wrapper">
                <pre className={`tool-card__output ${!showFullOutput && hasMore ? 'tool-card__output--truncated' : ''}`}>
                  {displayOutput}
                </pre>
                {hasMore && (
                  <button
                    className="tool-card__expand-btn"
                    onClick={(e) => {
                      e.stopPropagation()
                      setShowFullOutput(!showFullOutput)
                    }}
                    type="button"
                  >
                    {showFullOutput ? '收起内容' : `展开更多 (${tool.outputText.split('\n').length} 行)`}
                  </button>
                )}
              </div>
            </div>
          )}

          {/* 结果显示 */}
          {tool.resultText && (
            <div className="tool-card__section">
              <div className="tool-card__label">执行结果</div>
              <pre className="tool-card__result">{tool.resultText}</pre>
            </div>
          )}

          {/* 操作按钮 */}
          {onAction && (
            <div className="tool-card__actions">
              {tool.status === 'error' && (
                <button
                  className="tool-card__action-btn"
                  onClick={(e) => {
                    e.stopPropagation()
                    onAction('retry', tool)
                  }}
                  type="button"
                  title="重试执行"
                >
                  重试
                </button>
              )}
              
              {tool.status === 'approval_required' && (
                <button
                  className="tool-card__action-btn tool-card__action-btn--primary"
                  onClick={(e) => {
                    e.stopPropagation()
                    onAction('approve', tool)
                  }}
                  type="button"
                  title="批准执行"
                >
                  批准
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function detectToolType(name: string): ToolType {
  const lowerName = name.toLowerCase()
  if (lowerName.includes('read') || lowerName.includes('get') || lowerName.includes('load')) return 'read'
  if (lowerName.includes('edit') || lowerName.includes('update') || lowerName.includes('modify')) return 'edit'
  if (lowerName.includes('delete') || lowerName.includes('remove') || lowerName.includes('rm')) return 'delete'
  if (lowerName.includes('move') || lowerName.includes('rename') || lowerName.includes('mv')) return 'move'
  if (lowerName.includes('search') || lowerName.includes('find') || lowerName.includes('grep')) return 'search'
  if (lowerName.includes('execute') || lowerName.includes('run') || lowerName.includes('exec')) return 'execute'
  if (lowerName.includes('think') || lowerName.includes('reason') || lowerName.includes('plan')) return 'think'
  if (lowerName.includes('fetch') || lowerName.includes('request') || lowerName.includes('http')) return 'fetch'
  return 'other'
}

function truncateOutput(text: string, maxLines: number): { truncated: string; hasMore: boolean } {
  if (!text) return { truncated: '', hasMore: false }
  const lines = text.split('\n')
  if (lines.length <= maxLines) return { truncated: text, hasMore: false }
  return { truncated: lines.slice(0, maxLines).join('\n'), hasMore: true }
}

function formatArguments(argsText: string | undefined | null): string {
  if (!argsText) return ''
  try { return JSON.stringify(JSON.parse(argsText), null, 2) } catch { return argsText }
}