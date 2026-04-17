import { ChatErrorBoundary } from '@graphweave/ui'
import { ChatPage } from './pages/ChatPage'
import { ThemeProvider } from '@graphweave/hooks'

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
