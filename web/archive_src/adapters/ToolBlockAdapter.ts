/**
 * 工具块适配器
 * 将聚合后的工具状态转换为通用 UI 类型
 */

import type { UIToolContent } from '../types/ui/message'
import type { ToolStreamState } from './ToolStreamAggregator'

/**
 * 工具块适配器类
 */
export class ToolBlockAdapter {
  /**
   * 将工具状态转换为 UI 工具内容
   */
  static toUI(tool: ToolStreamState): UIToolContent {
    return {
      type: 'tool',
      id: tool.callId,
      name: tool.name,
      status: this.mapStatus(tool.status),
      argumentsText: tool.argumentsText,
      outputText: tool.outputText,
      resultText: tool.resultText,
      isError: tool.isError,
    }
  }

  /**
   * 映射 Loom 工具状态到 UI 工具状态
   */
  private static mapStatus(
    loomStatus: 'queued' | 'running' | 'done' | 'error'
  ): 'pending' | 'running' | 'success' | 'error' {
    const statusMap = {
      queued: 'pending' as const,
      running: 'running' as const,
      done: 'success' as const,
      error: 'error' as const,
    }
    return statusMap[loomStatus]
  }
}
