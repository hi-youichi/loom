import { ReactNode, useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'

interface TabPaneProps {
  tabId: string
  children: ReactNode
  className?: string
}

export function TabPane({ tabId, children, className }: TabPaneProps) {
  return (
    <div
      id={`panel-${tabId}`}
      role="tabpanel"
      aria-labelledby={`tab-${tabId}`}
      className={cn('tab-pane', className)}
    >
      {children}
    </div>
  )
}

interface TabContentProps {
  activeTab: string
  children: ReactNode
  animation?: 'fade' | 'slide' | 'scale' | 'none'
  className?: string
}

export function TabContent({ activeTab, children, animation = 'fade', className }: TabContentProps) {
  return (
    <div className={cn('tab-content-container', `tab-content--animation-${animation}`, className)}>
      {children}
    </div>
  )
}
