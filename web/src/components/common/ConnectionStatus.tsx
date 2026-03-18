import { memo } from 'react'
import type { WebSocketStatus } from '../../types/protocol/loom'

interface ConnectionStatusProps {
  status: WebSocketStatus
  className?: string
}

export const ConnectionStatus = memo(function ConnectionStatus({
  status,
  className = '',
}: ConnectionStatusProps) {
  if (status === 'connected') {
    return null
  }

  const statusConfig = {
    connecting: {
      label: '连接中',
      className: 'connection-status--connecting',
    },
    connected: {
      label: '已连接',
      className: 'connection-status--connected',
    },
    disconnected: {
      label: '未连接',
      className: 'connection-status--disconnected',
    },
    error: {
      label: '连接错误',
      className: 'connection-status--error',
    },
  }

  const config = statusConfig[status]

  return (
    <div
      className={`connection-status ${config.className} ${className}`}
      role="status"
      aria-live="polite"
      aria-label={`连接状态: ${config.label}`}
    >
      <span className="connection-status__indicator" aria-hidden="true" />
      <span className="connection-status__text">{config.label}</span>
    </div>
  )
})
