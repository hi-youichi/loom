import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  type ReactNode,
} from 'react'
import type { Theme } from './themeUtils'
import { getSystem, getInitial } from './themeUtils'

interface ThemeCtx {
  theme: Theme
  resolved: 'light' | 'dark'
  setTheme: (t: Theme) => void
}

const Ctx = createContext<ThemeCtx>({
  theme: 'system',
  resolved: 'light',
  setTheme: () => {},
})

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(getInitial)
  const [resolved, setResolved] = useState<'light' | 'dark'>(
    theme === 'system' ? getSystem() : theme,
  )

  const setTheme = useCallback(
    (t: Theme) => {
      setThemeState(t)
      try { localStorage.setItem('theme', t) } catch {
        // Ignore localStorage errors
      }
    },
    [],
  )

  useEffect(() => {
    const resolved = theme === 'system' ? getSystem() : theme
    setResolved(resolved)
    document.documentElement.classList.toggle('dark', resolved === 'dark')
  }, [theme])

  useEffect(() => {
    if (theme !== 'system') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => {
      const resolved = getSystem()
      setResolved(resolved)
      document.documentElement.classList.toggle('dark', resolved === 'dark')
    }
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  }, [theme])

  return <Ctx.Provider value={{ theme, resolved, setTheme }}>{children}</Ctx.Provider>
}

export function useTheme() {
  return useContext(Ctx)
}