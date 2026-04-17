import { useTheme } from '@graphweave/hooks'
import type { IconName } from '@graphweave/types'
import { ToolIcon } from './ToolIcon'

const OPTIONS: { value: 'light' | 'dark' | 'system'; icon: IconName }[] = [
  { value: 'light', icon: 'sun' },
  { value: 'dark', icon: 'moon' },
  { value: 'system', icon: 'monitor' },
]

export function ThemeToggle() {
  const { theme, setTheme } = useTheme()

  return (
    <div
      style={{
        display: 'inline-flex',
        border: '1px solid var(--tool-border)',
        borderRadius: 6,
        overflow: 'hidden',
      }}
    >
      {OPTIONS.map((opt) => {
        const active = theme === opt.value
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => setTheme(opt.value)}
            title={opt.value.charAt(0).toUpperCase() + opt.value.slice(1)}
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: 32,
              height: 28,
              border: 'none',
              background: active ? 'var(--tool-surface-hover)' : 'transparent',
              color: active ? 'var(--tool-text)' : 'var(--tool-text-muted)',
              cursor: 'pointer',
              transition: 'background 0.15s, color 0.15s',
            }}
          >
            <ToolIcon name={opt.icon} size={14} />
          </button>
        )
      })}
    </div>
  )
}
