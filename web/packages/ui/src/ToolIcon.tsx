import { ICON_MAP, type IconName } from '@graphweave/types'

export function ToolIcon({ name, size = 16, className, style }: {
  name: IconName
  size?: number
  className?: string
  style?: React.CSSProperties
}) {
  const Icon = ICON_MAP[name]
  if (!Icon) return null
  return <Icon size={size} className={className} style={style} />
}
