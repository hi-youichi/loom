import { ChatErrorBoundary } from '../components/error/ErrorBoundary'
import { ChatLayout } from '../components/layout/ChatLayout'
import { MessageList } from '../components/chat/MessageList'
import { ConnectionStatus } from '../components/common/ConnectionStatus'
import { MessageComposer } from '../components/MessageComposer'
import { ThinkIndicator } from '../components/ThinkIndicator'
import { useChat } from '../hooks/useChat'

export function ChatPage() {
  const {
    messages,
    isStreaming,
    thinkingLines,
    connectionStatus,
    error,
    sendMessage,
  } = useChat()

  const handleRetry = () => {
    window.location.reload()
  }

  if (error) {
    return (
      <div className="error-page">
        <h2>出现错误</h2>
        <p>{error}</p>
        <button onClick={handleRetry}>重新加载</button>
      </div>
    )
  }

  return (
    <ChatErrorBoundary>
      <ChatLayout>
        <ConnectionStatus status={connectionStatus} />
        
        <MessageList messages={messages} />
        
        {thinkingLines.length > 0 ? (
          <ThinkIndicator lines={thinkingLines} active={isStreaming} />
        ) : null}
        
        <MessageComposer
          disabled={isStreaming || connectionStatus !== 'connected'}
          onSend={sendMessage}
        />
      </ChatLayout>
    </ChatErrorBoundary>
  )
}
