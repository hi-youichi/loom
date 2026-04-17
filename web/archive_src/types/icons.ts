import type { LucideIcon } from 'lucide-react'
import {
  FileText,
  Pencil,
  Trash2,
  FolderInput,
  Search,
  Play,
  Brain,
  Globe,
  Settings,
  Loader,
  CheckCircle2,
  XCircle,
  Lock,
  Hourglass,
  Sun,
  Moon,
  Monitor,
} from 'lucide-react'
import type { ToolType } from './chat'

export type IconName =
  | 'file-text'
  | 'pencil'
  | 'trash-2'
  | 'folder-input'
  | 'search'
  | 'play'
  | 'brain'
  | 'globe'
  | 'settings'
  | 'loader'
  | 'check-circle'
  | 'x-circle'
  | 'lock'
  | 'hourglass'
  | 'sun'
  | 'moon'
  | 'monitor'

const ICON_MAP: Record<IconName, LucideIcon> = {
  'file-text': FileText,
  'pencil': Pencil,
  'trash-2': Trash2,
  'folder-input': FolderInput,
  'search': Search,
  'play': Play,
  'brain': Brain,
  'globe': Globe,
  'settings': Settings,
  'loader': Loader,
  'check-circle': CheckCircle2,
  'x-circle': XCircle,
  'lock': Lock,
  'hourglass': Hourglass,
  'sun': Sun,
  'moon': Moon,
  'monitor': Monitor,
}

export const TOOL_TYPE_ICONS: Record<ToolType, IconName> = {
  read: 'file-text',
  edit: 'pencil',
  delete: 'trash-2',
  move: 'folder-input',
  search: 'search',
  execute: 'play',
  think: 'brain',
  fetch: 'globe',
  other: 'settings',
}

export const TOOL_STATUS_ICONS: Record<string, IconName> = {
  queued: 'hourglass',
  running: 'loader',
  done: 'check-circle',
  error: 'x-circle',
  approval_required: 'lock',
}

export { ICON_MAP }
