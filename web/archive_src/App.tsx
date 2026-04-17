import { ChatErrorBoundary } from './components/error/ErrorBoundary'
import { ChatPage } from './pages/ChatPage'
import { ThemeProvider } from './hooks/useTheme'

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
