/**
 * TextMessage 组件测试
 */

import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { TextMessage } from '../../components/chat/TextMessage'

describe('TextMessage', () => {
  it('应该渲染文本内容', () => {
    render(
      <TextMessage 
        content={{
          type: 'text',
          text: 'Hello World'
        }}
      />
    )
    
    expect(screen.getByText('Hello World')).toBeInTheDocument()
  })

  it('应该支持自定义className', () => {
    const { container } = render(
      <TextMessage 
        content={{
          type: 'text',
          text: 'Test'
        }}
        className="custom-class"
      />
    )
    
    expect(container.firstChild).toHaveClass('custom-class')
  })

  it('应该正确处理多行文本', () => {
    render(
      <TextMessage 
        content={{
          type: 'text',
          text: 'Line 1\nLine 2\nLine 3'
        }}
      />
    )
    
    expect(screen.getByText(/Line 1/)).toBeInTheDocument()
    expect(screen.getByText(/Line 2/)).toBeInTheDocument()
    expect(screen.getByText(/Line 3/)).toBeInTheDocument()
  })

  it('应该正确处理空文本', () => {
    const { container } = render(
      <TextMessage 
        content={{
          type: 'text',
          text: ''
        }}
      />
    )
    
    expect(container.querySelector('.text-message')).toBeInTheDocument()
  })

  it('应该正确处理长文本', () => {
    const longText = 'A'.repeat(1000)
    
    render(
      <TextMessage 
        content={{
          type: 'text',
          text: longText
        }}
      />
    )
    
    expect(screen.getByText(longText)).toBeInTheDocument()
  })

  it('应该正确处理特殊字符', () => {
    const specialText = '<script>alert("XSS")</script> & "quotes" \'apostrophes\''
    
    render(
      <TextMessage 
        content={{
          type: 'text',
          text: specialText
        }}
      />
    )
    
    expect(screen.getByText(specialText)).toBeInTheDocument()
  })

  it('应该正确处理emoji', () => {
    const emojiText = 'Hello 👋 World 🌍'
    
    render(
      <TextMessage 
        content={{
          type: 'text',
          text: emojiText
        }}
      />
    )
    
    expect(screen.getByText(emojiText)).toBeInTheDocument()
  })
})
