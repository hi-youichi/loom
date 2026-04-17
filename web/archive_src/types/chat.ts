export type MessageRole = 'user' | 'assistant'

export type TextBlock = {
  id: string
  type: 'text'
  text: string
}

export type ToolType = 
  | 'read'      // 读取文件/数据
  | 'edit'      // 修改文件/内容  
  | 'delete'    // 删除文件/数据
  | 'move'      // 移动/重命名文件
  | 'search'    // 搜索信息
  | 'execute'   // 运行命令/代码
  | 'think'     // 内部推理/规划
  | 'fetch'     // 获取外部数据
  | 'other'     // 其他工具类型

export type ToolStatus = 'queued' | 'running' | 'done' | 'error' | 'approval_required'

export type ToolBlock = {
  id: string
  type: 'tool'
  callId: string
  name: string
  toolType?: ToolType
  status: ToolStatus
  argumentsText: string
  outputText: string
  resultText: string
  isError: boolean
  timestamp?: string
  duration?: number
}

export type MessageBlock = TextBlock | ToolBlock

export type Message = {
  id: string
  role: MessageRole
  blocks: MessageBlock[]
  createdAt: string
}
