import { ChatErrorBoundary } from './components/error/ErrorBoundary'
import { ChatPage } from './pages/ChatPage-new'
import { DashboardDemo } from './pages/DashboardDemo'
import { ThemeProvider } from './hooks/useTheme'
import { useState } from 'react'

function App() {
  const [showDashboard, setShowDashboard] = useState(true)

  return (
    <ThemeProvider>
      <ChatErrorBoundary>
        {/* Debug toggle for development */}
        <div style={{ position: 'fixed', top: 0, right: 0, zIndex: 9999, padding: '10px' }}>
          <button
            onClick={() => setShowDashboard(!showDashboard)}
            style={{
              padding: '8px 16px',
              background: '#007bff',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: 'pointer',
              fontSize: '14px'
            }}
          >
            {showDashboard ? '切换到聊天' : '切换到仪表板'}
          </button>
        </div>
        
        {showDashboard ? <DashboardDemo /> : <ChatPage />}
      </ChatErrorBoundary>
    </ThemeProvider>
  )
}

export default App

