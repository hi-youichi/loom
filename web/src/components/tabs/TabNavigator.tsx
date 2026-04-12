import { useEffect, useRef, useState, Children, ReactElement } from 'react'
import { cn } from '@/lib/utils'

export type TabState = string

export interface TabConfig {
  id: TabState
  label: string
  icon: string
  badge?: number
  disabled?: boolean
}

interface TabNavigatorProps {
  tabs: TabConfig[]
  activeTab: TabState
  onTabChange: (tab: TabState) => void
  variant?: 'default' | 'pills' | 'underline'
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export function TabNavigator({
  tabs,
  activeTab,
  onTabChange,
  variant = 'default',
  size = 'md',
  className
}: TabNavigatorProps) {
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([])
  const [focusedIndex, setFocusedIndex] = useState<number>(-1)

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (focusedIndex === -1) return

      switch (e.key) {
        case 'ArrowRight':
          e.preventDefault()
          const nextIndex = (focusedIndex + 1) % tabs.length
          setFocusedIndex(nextIndex)
          tabRefs.current[nextIndex]?.focus()
          break
        case 'ArrowLeft':
          e.preventDefault()
          const prevIndex = (focusedIndex - 1 + tabs.length) % tabs.length
          setFocusedIndex(prevIndex)
          tabRefs.current[prevIndex]?.focus()
          break
        case 'Home':
          e.preventDefault()
          setFocusedIndex(0)
          tabRefs.current[0]?.focus()
          break
        case 'End':
          e.preventDefault()
          setFocusedIndex(tabs.length - 1)
          tabRefs.current[tabs.length - 1]?.focus()
          break
        case 'Enter':
        case ' ':
          e.preventDefault()
          if (!tabs[focusedIndex].disabled) {
            onTabChange(tabs[focusedIndex].id)
          }
          break
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [focusedIndex, tabs, onTabChange])

  const handleTabClick = (index: number, tab: TabConfig) => {
    if (tab.disabled) return
    setFocusedIndex(index)
    onTabChange(tab.id)
  }

  const handleTabFocus = (index: number) => {
    setFocusedIndex(index)
  }

  return (
    <div className={cn('tab-navigator', `tab-navigator--${variant}`, `tab-navigator--${size}`, className)}>
      {tabs.map((tab, index) => (
        <button
          key={tab.id}
          ref={(el) => (tabRefs.current[index] = el)}
          className={cn(
            'tab-item',
            activeTab === tab.id && 'tab-item--active',
            tab.disabled && 'tab-item--disabled',
            focusedIndex === index && 'tab-item--focused'
          )}
          onClick={() => handleTabClick(index, tab)}
          onFocus={() => handleTabFocus(index)}
          disabled={tab.disabled}
          aria-selected={activeTab === tab.id}
          aria-controls={`panel-${tab.id}`}
          role="tab"
          tabIndex={activeTab === tab.id ? 0 : -1}
          type="button"
        >
          <span className="tab-icon">{tab.icon}</span>
          <span className="tab-label">{tab.label}</span>
          {tab.badge !== undefined && tab.badge > 0 && (
            <span className="tab-badge">{tab.badge}</span>
          )}
        </button>
      ))}
    </div>
  )
}
