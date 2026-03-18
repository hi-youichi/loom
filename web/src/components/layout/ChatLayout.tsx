import type { ReactNode } from 'react'

interface ChatLayoutProps {
  children: ReactNode
}

export function ChatLayout({ children }: ChatLayoutProps) {
  return (
    <div className="chat-layout">
      <div className="chat-layout__container">
        {children}
      </div>
    </div>
  )
}
