import { describe, it, expect } from 'vitest'
import { formatTime, formatDuration, formatRelativeTime, formatFileSize } from '../../utils/format'

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
      expect(() => formatTime(timestamp)).toThrow()
    })
  })

  describe('formatDuration', () => {
    it('应该格式化毫秒', () => {
      expect(formatDuration(500)).toBe('500ms')
      expect(formatDuration(999)).toBe('999ms')
    })

    it('应该格式化秒', () => {
      expect(formatDuration(1000)).toBe('1.0s')
      expect(formatDuration(1500)).toBe('1.5s')
      expect(formatDuration(59999)).toBe('60.0s')
    })

    it('应该格式化分钟', () => {
      expect(formatDuration(60000)).toBe('1m 0s')
      expect(formatDuration(90000)).toBe('1m 30s')
    })

    it('应该处理0', () => {
      expect(formatDuration(0)).toBe('0ms')
    })

    it('应该处理负数', () => {
      expect(formatDuration(-100)).toBe('-100ms')
    })
  })

  describe('formatRelativeTime', () => {
    it('应该返回"刚刚"对于小于60秒', () => {
      const now = new Date().toISOString()
      expect(formatRelativeTime(now)).toBe('刚刚')
    })

    it('应该返回分钟前对于小于60分钟', () => {
      const fiveMinutesAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString()
      expect(formatRelativeTime(fiveMinutesAgo)).toBe('5分钟前')
    })

    it('应该返回小时前对于小于24小时', () => {
      const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString()
      expect(formatRelativeTime(twoHoursAgo)).toBe('2小时前')
    })

    it('应该返回天前对于小于7天', () => {
      const threeDaysAgo = new Date(Date.now() - 3 * 24 * 60 * 60 * 1000).toISOString()
      expect(formatRelativeTime(threeDaysAgo)).toBe('3天前')
    })

    it('应该返回格式化时间对于超过7天', () => {
      const tenDaysAgo = new Date(Date.now() - 10 * 24 * 60 * 60 * 1000).toISOString()
      const result = formatRelativeTime(tenDaysAgo)
      // 应该调用 formatTime 返回时间格式
      expect(result).toMatch(/\d{2}:\d{2}/)
    })
  })

  describe('formatFileSize', () => {
    it('应该处理0字节', () => {
      expect(formatFileSize(0)).toBe('0 B')
    })

    it('应该格式化字节', () => {
      expect(formatFileSize(500)).toBe('500 B')
    })

    it('应该格式化KB', () => {
      expect(formatFileSize(1024)).toBe('1 KB')
      expect(formatFileSize(1536)).toBe('1.5 KB')
    })

    it('应该格式化MB', () => {
      expect(formatFileSize(1024 * 1024)).toBe('1 MB')
    })

    it('应该格式化GB', () => {
      expect(formatFileSize(1024 * 1024 * 1024)).toBe('1 GB')
    })
  })
})
