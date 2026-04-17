import { describe, it, expect } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useFileTree } from '../../components/file-tree/useFileTree'

describe('useFileTree', () => {
  it('throws error when used outside FileTreeProvider', () => {
    expect(() => {
      renderHook(() => useFileTree())
    }).toThrow('useFileTree must be used within FileTreeProvider')
  })
})
