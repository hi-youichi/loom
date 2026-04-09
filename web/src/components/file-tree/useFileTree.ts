import { useContext } from 'react'
import { FileTreeContext } from './FileTreeContext'

export function useFileTree() {
  const context = useContext(FileTreeContext)
  if (!context) {
    throw new Error('useFileTree must be used within FileTreeProvider')
  }
  return context
}
