import { ToolType } from './chat'

export interface ToolTypeInfo {
  icon: string
  label: string
  color: string
  description: string
}

export const TOOL_TYPE_INFO: Record<ToolType, ToolTypeInfo> = {
  read: {
    icon: '📄',
    label: '读取',
    color: '#2196F3',
    description: '读取文件或数据'
  },
  edit: {
    icon: '✏️',
    label: '编辑',
    color: '#FF9800',
    description: '修改文件或内容'
  },
  delete: {
    icon: '🗑️',
    label: '删除',
    color: '#F44336',
    description: '删除文件或数据'
  },
  move: {
    icon: '🔄',
    label: '移动',
    color: '#9C27B0',
    description: '移动或重命名文件'
  },
  search: {
    icon: '🔍',
    label: '搜索',
    color: '#4CAF50',
    description: '搜索信息'
  },
  execute: {
    icon: '▶️',
    label: '执行',
    color: '#1976D2',
    description: '运行命令或代码'
  },
  think: {
    icon: '🤔',
    label: '思考',
    color: '#757575',
    description: '内部推理或规划'
  },
  fetch: {
    icon: '🌐',
    label: '获取',
    color: '#00BCD4',
    description: '获取外部数据'
  },
  other: {
    icon: '⚙️',
    label: '其他',
    color: '#9E9E9E',
    description: '其他工具类型'
  }
}

export const TOOL_STATUS_INFO = {
  queued: { icon: '⏳', label: '等待中', color: '#999999' },
  running: { icon: '🔄', label: '运行中', color: '#2196F3' },
  done: { icon: '✅', label: '已完成', color: '#4CAF50' },
  error: { icon: '❌', label: '错误', color: '#F44336' },
  approval_required: { icon: '🔒', label: '需审批', color: '#FF9800' }
}