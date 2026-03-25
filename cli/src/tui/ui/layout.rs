use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// 布局计算器
pub struct LayoutCalculator;

impl LayoutCalculator {
    /// 计算主布局
    /// 
    /// 返回 (header, main_area, input_bar)
    pub fn calculate_main_layout(area: Rect) -> (Rect, Rect, Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header - 固定 3 行
                Constraint::Min(10),    // Main area - 最小 10 行
                Constraint::Length(3),  // Input bar - 固定 3 行
            ])
            .split(area);

        (chunks[0], chunks[1], chunks[2])
    }

    /// 计算主区域的水平布局（Agent 网格和 Session 面板）
    /// 
    /// 返回 (agent_grid, session_panel)
    pub fn calculate_horizontal_layout(area: Rect) -> (Rect, Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),  // Agent 网格 - 40%
                Constraint::Percentage(60),  // Session 面板 - 60%
            ])
            .split(area);

        (chunks[0], chunks[1])
    }
}
