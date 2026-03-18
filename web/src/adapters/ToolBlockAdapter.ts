/**
 * 工具块适配器
 * 将 Loom 协议的工具数据转换为通用 UI 类型
 */

import type { LoomAssistantToolEvent } from '../types/protocol/loom'
import type { UIToolContent } from '../types/ui/message'

/**
 * 工具块适配器类
 */
export class ToolBlockAdapter {
  /**
   * 将 Loom 工具事件转换为 UI 工具内容
   */
  static toUI(event: LoomAssistantToolEvent): UIToolContent {
    return {
      type: 'tool',
      id: event.callId,
      name: event.name,
      status: this.mapStatus(event.status),
      argumentsText: event.argumentsText,
      outputText: event.outputText,
      resultText: event.resultText,
      isError: event.isError,
    }
  }

  /**
   * 映射 Loom 工具状态到 UI 工具状态
   */
  private static mapStatus(
    loomStatus: 'queued' | 'running' | 'done' | 'error' | 'approval_required'
  ): 'pending' | 'running' | 'success' | 'error' {
    const statusMap = {
      queued: 'pending' as const,
      running: 'running' as const,
      done: 'success' as const,
      error: 'error' as const,
      approval_required: 'pending' as const,
    }
    return statusMap[loomStatus]
  }
}
