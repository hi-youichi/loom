export type Theme = 'light' | 'dark' | 'system'

export function getSystem(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light'
}

export function getInitial(): Theme {
  try {
    return (localStorage.getItem('theme') as Theme) || 'system'
  } catch {
    return 'system'
  }
}
