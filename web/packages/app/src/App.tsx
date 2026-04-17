import { ChatErrorBoundary } from '@loom/ui'
import { ChatPage } from './pages/ChatPage'
import { ThemeProvider } from '@loom/hooks'

function App() {
  return (
    <ThemeProvider>
      <ChatErrorBoundary>
        <ChatPage />
      </ChatErrorBoundary>
    </ThemeProvider>
  )
}

export default App
