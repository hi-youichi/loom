import { ChatErrorBoundary } from './components/error/ErrorBoundary'
import { ChatPage } from './pages/ChatPage-new'

function App() {
  return (
    <ChatErrorBoundary>
      <ChatPage />
    </ChatErrorBoundary>
  )
}

export default App
