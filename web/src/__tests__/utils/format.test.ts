import { describe, it, expect } from 'vitest'
import { formatTime, formatDuration, truncateText, classNames } from '../utils/format'

describe('format utilities', () => {
  describe('formatTime', () => {
    it('应该正确格式化时间', () => {
      const timestamp = '2024-01-15T10:30:00Z'
      const result = formatTime(timestamp)
      expect(result).toMatch(/\d{2}:\d{2}/)
    })

    it('应该处理不同的时区', () => {
      const timestamp = '2024-01-15T10:30:00+08:00'
      const result = formatTime(timestamp)
      expect(result).toBeDefined()
    })

    it('应该处理无效时间', () => {
      const timestamp = 'invalid'
      const result = formatTime(timestamp)
      expect(result).toBe('Invalid Date')
    })
  })

  describe('formatDuration', () => {
    it('应该格式化毫秒', () => {
      expect(formatDuration(500)).toBe('500ms')
      expect(formatDuration(999)).toBe('999ms')
    })

    it('应该格式化秒', () => {
      expect(formatDuration(1000)).toBe('1.00s')
      expect(formatDuration(1500)).toBe('1.50s')
      expect(formatDuration(59999)).toBe('59.99s')
    })

    it('应该格式化分钟', () => {
      expect(formatDuration(60000)).toBe('1.00m')
      expect(formatDuration(90000)).toBe('1.50m')
    })

    it('应该处理0', () => {
      expect(formatDuration(0)).toBe('0ms')
    })

    it('应该处理负数', () => {
      expect(formatDuration(-100)).toBe('-100ms')
    })
  })

  describe('truncateText', () => {
    it('应该截断长文本', () => {
      const text = '这是一段很长的文本需要被截断'
      const result = truncateText(text, 10)
      expect(result.length).toBe(13) // 10 + '...'
      expect(result).toBe('这是一段很长的文本...')
    })

    it('应该不截断短文本', () => {
      const text = '短文本'
      const result = truncateText(text, 10)
      expect(result).toBe(text)
    })

    it('应该处理空字符串', () => {
      expect(truncateText('', 10)).toBe('')
    })

    it('应该使用默认长度', () => {
      const text = 'a'.repeat(150)
      const result = truncateText(text)
      expect(result.length).toBe(103) // 100 + '...'
    })
  })

  describe('classNames', () => {
    it('应该合并类名', () => {
      const result = classNames('foo', 'bar')
      expect(result).toBe('foo bar')
    })

    it('应该过滤假值', () => {
      const result = classNames('foo', false, 'bar', null, 'baz', undefined)
      expect(result).toBe('foo bar baz')
    })

    it('应该支持对象语法', () => {
      const result = classNames({
        foo: true,
        bar: false,
        baz: true
      })
      expect(result).toBe('foo baz')
    })

    it('应该支持混合语法', () => {
      const result = classNames(
        'foo',
        { bar: true },
        'baz',
        { qux: false }
      )
      expect(result).toBe('foo bar baz')
    })

    it('应该处理空输入', () => {
      expect(classNames()).toBe('')
      expect(classNames('')).toBe('')
    })
  })
})
