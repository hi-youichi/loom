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

    /// 计算 Agent 网格布局（根据 Agent 数量动态计算）
    /// 
    /// 返回每个 Agent 卡片的区域
    pub fn calculate_agent_grid(area: Rect, agent_count: usize) -> Vec<Rect> {
        if agent_count == 0 {
            return vec![];
        }

        // 计算列数（每行最多 2 个）
        let cols = if agent_count == 1 { 1 } else { 2 };
        
        // 计算行数
        let rows = (agent_count + cols - 1) / cols;

        // 创建行布局
        let row_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(8); rows])
            .split(area);

        // 为每行创建列布局
        let mut result = Vec::new();
        for row_chunk in row_chunks.iter() {
            let col_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Percentage(50); cols])
                .split(*row_chunk);
            
            result.extend(col_chunks.iter().cloned());
        }

        result
    }
}
