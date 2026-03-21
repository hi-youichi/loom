//! Virtual scrolling implementation for efficient rendering of large lists.
//!
//! This module provides virtual scrolling functionality to render only visible items,
//! significantly improving performance for large message lists.

use ratatui::layout::Rect;

/// Configuration for virtual scrolling
#[derive(Debug, Clone)]
pub struct VirtualScrollConfig {
    /// Height of each item (can be variable, but we use average for estimation)
    pub item_height: usize,
    /// Overscan count - render extra items above and below visible area
    pub overscan: usize,
}

impl Default for VirtualScrollConfig {
    fn default() -> Self {
        Self {
            item_height: 3, // Average message height
            overscan: 5,    // Render 5 extra items above and below
        }
    }
}

/// Virtual scroll state
#[derive(Debug, Clone)]
pub struct VirtualScroll {
    /// Configuration
    config: VirtualScrollConfig,
    /// Current scroll offset (in items, not pixels)
    offset: usize,
    /// Total number of items
    total_items: usize,
    /// Viewport height
    viewport_height: usize,
}

impl VirtualScroll {
    /// Create a new virtual scroll instance
    pub fn new(config: VirtualScrollConfig) -> Self {
        Self {
            config,
            offset: 0,
            total_items: 0,
            viewport_height: 0,
        }
    }

    /// Update total number of items
    pub fn set_total_items(&mut self, total: usize) {
        self.total_items = total;
    }

    /// Update viewport height
    pub fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height;
    }

    /// Get current scroll offset
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Scroll up by one item
    pub fn scroll_up(&mut self) {
        if self.offset > 0 {
            self.offset -= 1;
        }
    }

    /// Scroll down by one item
    pub fn scroll_down(&mut self) {
        let max_offset = self.max_offset();
        if self.offset < max_offset {
            self.offset += 1;
        }
    }

    /// Scroll to a specific item
    pub fn scroll_to(&mut self, index: usize) {
        let max_offset = self.max_offset();
        self.offset = index.min(max_offset);
    }

    /// Scroll to make an item visible
    pub fn scroll_into_view(&mut self, index: usize) {
        let visible_range = self.visible_range();
        
        if index < visible_range.start {
            // Item is above visible area
            self.offset = index.saturating_sub(self.config.overscan);
        } else if index >= visible_range.end {
            // Item is below visible area
            let visible_count = self.visible_count();
            self.offset = (index + self.config.overscan)
                .saturating_sub(visible_count.saturating_sub(1))
                .min(self.max_offset());
        }
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.offset = self.max_offset();
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
    }

    /// Get the range of visible items (including overscan)
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        if self.total_items == 0 {
            return 0..0;
        }

        let visible_count = self.visible_count();
        
        let start = self.offset.saturating_sub(self.config.overscan);
        let end = (self.offset + visible_count + self.config.overscan).min(self.total_items);

        start..end
    }

    /// Get the number of visible items (excluding overscan)
    pub fn visible_count(&self) -> usize {
        if self.viewport_height == 0 || self.config.item_height == 0 {
            return 0;
        }
        
        self.viewport_height / self.config.item_height
    }

    /// Check if we're at the bottom
    pub fn is_at_bottom(&self) -> bool {
        self.offset >= self.max_offset()
    }

    /// Check if we're at the top
    pub fn is_at_top(&self) -> bool {
        self.offset == 0
    }

    /// Get the maximum scroll offset
    fn max_offset(&self) -> usize {
        if self.total_items == 0 {
            return 0;
        }

        let visible_count = self.visible_count();
        if visible_count >= self.total_items {
            return 0;
        }

        self.total_items.saturating_sub(visible_count)
    }
}

/// Variable height virtual scroll for items with different heights
#[derive(Debug, Clone)]
pub struct VariableVirtualScroll {
    /// Current scroll offset (in items)
    offset: usize,
    /// Total number of items
    total_items: usize,
    /// Viewport height in lines
    viewport_height: usize,
    /// Cached item heights
    item_heights: Vec<usize>,
    /// Cached cumulative heights
    cumulative_heights: Vec<usize>,
    /// Overscan count
    overscan: usize,
}

impl VariableVirtualScroll {
    /// Create a new variable height virtual scroll
    pub fn new() -> Self {
        Self {
            offset: 0,
            total_items: 0,
            viewport_height: 0,
            item_heights: Vec::new(),
            cumulative_heights: Vec::new(),
            overscan: 5,
        }
    }

    /// Set item heights
    pub fn set_item_heights(&mut self, heights: Vec<usize>) {
        self.item_heights = heights.clone();
        self.total_items = heights.len();
        
        // Calculate cumulative heights
        self.cumulative_heights = Vec::with_capacity(heights.len());
        let mut cumsum = 0;
        for height in heights {
            cumsum += height;
            self.cumulative_heights.push(cumsum);
        }
    }

    /// Update viewport height
    pub fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height;
    }

    /// Get current scroll offset
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Scroll up by one item
    pub fn scroll_up(&mut self) {
        if self.offset > 0 {
            self.offset -= 1;
        }
    }

    /// Scroll down by one item
    pub fn scroll_down(&mut self) {
        let max_offset = self.max_offset();
        if self.offset < max_offset {
            self.offset += 1;
        }
    }

    /// Scroll to a specific item
    pub fn scroll_to(&mut self, index: usize) {
        let max_offset = self.max_offset();
        self.offset = index.min(max_offset);
    }

    /// Scroll to make an item visible
    pub fn scroll_into_view(&mut self, index: usize) {
        let visible_range = self.visible_range();
        
        if index < visible_range.start {
            self.offset = index.saturating_sub(self.overscan);
        } else if index >= visible_range.end {
            self.offset = index.saturating_sub(self.visible_count().saturating_sub(1));
            self.offset = self.offset.min(self.max_offset());
        }
    }

    /// Get the range of visible items (including overscan)
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        if self.total_items == 0 {
            return 0..0;
        }

        let start = self.offset.saturating_sub(self.overscan);
        let end = (self.offset + self.visible_count() + self.overscan).min(self.total_items);

        start..end
    }

    /// Get approximate number of visible items
    fn visible_count(&self) -> usize {
        if self.viewport_height == 0 || self.item_heights.is_empty() {
            return 10; // Default
        }

        // Estimate based on average height
        let avg_height: usize = if self.cumulative_heights.is_empty() {
            3
        } else {
            self.cumulative_heights.last().unwrap_or(&1) / self.item_heights.len().max(1)
        };

        self.viewport_height / avg_height.max(1)
    }

    /// Get scroll position in lines (for rendering)
    pub fn scroll_lines(&self) -> usize {
        if self.offset == 0 {
            return 0;
        }
        
        self.cumulative_heights
            .get(self.offset.saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }

    /// Check if we're at the bottom
    pub fn is_at_bottom(&self) -> bool {
        self.offset >= self.max_offset()
    }

    /// Get the maximum scroll offset
    fn max_offset(&self) -> usize {
        if self.total_items == 0 {
            return 0;
        }

        self.total_items.saturating_sub(1)
    }
}

impl Default for VariableVirtualScroll {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_scroll_basic() {
        let mut scroll = VirtualScroll::new(VirtualScrollConfig::default());
        scroll.set_total_items(100);
        scroll.set_viewport_height(30); // 10 items visible
        
        assert_eq!(scroll.visible_count(), 10);
        assert!(scroll.is_at_top());
        assert!(!scroll.is_at_bottom());
        
        scroll.scroll_down();
        assert_eq!(scroll.offset(), 1);
        
        scroll.scroll_to(50);
        assert_eq!(scroll.offset(), 50);
        
        scroll.scroll_to_bottom();
        assert!(scroll.is_at_bottom());
    }

    #[test]
    fn test_virtual_scroll_visible_range() {
        let mut scroll = VirtualScroll::new(VirtualScrollConfig {
            item_height: 3,
            overscan: 2,
        });
        scroll.set_total_items(100);
        scroll.set_viewport_height(30); // 10 items visible
        
        scroll.scroll_to(10);
        let range = scroll.visible_range();
        assert_eq!(range.start, 8);  // 10 - 2 (overscan)
        assert!(range.end > 10);
    }
}
