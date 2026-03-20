//! Agent 网格组件 - 显示所有 Agent 的状态卡片

use ratatui::{
    layout::{Constraint, Direction, Grid, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::{
    data::{AgentId, AgentInfo, AgentStatus},
    style::Theme,
};

/// Agent 网格渲染器
pub struct AgentGrid<'a> {
    agents: &'a [(AgentId, AgentInfo)],
    selected: Option<&'a AgentId>,
    theme: &'a Theme,
}

impl<'a> AgentGrid<'a> {
    pub fn new(
        agents: &'a [(AgentId, AgentInfo)],
        selected: Option<&'a AgentId>,
        theme: &'a Theme,
    ) -> Self {
        Self {
            agents,
            selected,
            theme,
        }
    }

    /// 渲染 Agent 网格
    pub fn render(self, frame: &mut Frame, area: Rect) {
        if area.height < 5 || area.width < 20 {
            return; // 区域太小，无法渲染
        }

        if self.agents.is_empty() {
            // 显示空状态
            let empty = Paragraph::new("No agents running")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(empty, area);
            return;
        }

        // 计算网格布局（最多 2 列）
        let cols = if area.width > 80 { 2 } else { 1 };
        let rows = (self.agents.len() + cols - 1) / cols;

        // 创建网格约束
        let constraints: Vec<Constraint> = (0..rows)
            .map(|_| Constraint::Min(8))
            .collect();

        // 使用 Grid 布局
        let grid = Grid::new(rows, cols)
            .constraints(constraints.as_slice())
            .horizontal_spacing(1)
            .vertical_spacing(1);

        // 渲染每个 Agent 卡片
        for (idx, (id, agent)) in self.agents.iter().enumerate() {
            let row = idx / cols;
            let col = idx % cols;
            
            if let Some(cell_area) = grid.cell_area(row, col, area) {
                self.render_agent_card(frame, cell_area, id, agent);
            }
        }
    }

    /// 渲染单个 Agent 卡片
    fn render_agent_card(
        &self,
        frame: &mut Frame,
        area: Rect,
        id: &AgentId,
        agent: &AgentInfo,
    ) {
        let is_selected = self.selected.map_or(false, |s| s == id);

        // 确定边框颜色
        let border_color = if is_selected {
            self.theme.accent
        } else {
            Color::DarkGray
        };

        // 确定状态颜色和图标
        let (status_icon, status_color) = match agent.status {
            AgentStatus::Running => ("●", self.theme.success),
            AgentStatus::Completed => ("✓", self.theme.info),
            AgentStatus::Error => ("✗", self.theme.error),
        };

        // 创建卡片标题
        let title = Line::from(vec![
            Span::styled(
                format!("{} ", status_icon),
                Style::default().fg(status_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                agent.name.as_str(),
                Style::default()
                    .fg(if is_selected {
                        self.theme.accent
                    } else {
                        Color::White
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        // 创建卡片块
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_type(ratatui::widgets::BorderType::Rounded);

        // 准备内容
        let mut lines = Vec::new();

        // 任务描述（截断）
        let task_text = if agent.task.len() > area.width as usize - 4 {
            format!("{}...", &agent.task[..area.width as usize - 7])
        } else {
            agent.task.clone()
        };
        lines.push(Line::from(Span::styled(
            task_text,
            Style::default().fg(Color::Gray),
        )));

        // 当前节点
        if let Some(node) = &agent.current_node {
            lines.push(Line::from(vec![
                Span::styled("Node: ", Style::default().fg(Color::DarkGray)),
                Span::styled(node.as_str(), Style::default().fg(self.theme.info)),
            ]));
        }

        // 进度消息
        if let Some(msg) = &agent.progress_message {
            let msg_text = if msg.len() > area.width as usize - 4 {
                format!("{}...", &msg[..area.width as usize - 7])
            } else {
                msg.clone()
            };
            lines.push(Line::from(Span::styled(
                msg_text,
                Style::default().fg(Color::Yellow),
            )));
        }

        // 结果或错误
        match &agent.result {
            Some(result) => {
                let text = if result.len() > 30 {
                    format!("Result: {}...", &result[..27])
                } else {
                    format!("Result: {}", result)
                };
                lines.push(Line::from(Span::styled(
                    text,
                    Style::default().fg(self.theme.success),
                )));
            }
            None => {
                if let Some(error) = &agent.error {
                    let text = if error.len() > 30 {
                        format!("Error: {}...", &error[..27])
                    } else {
                        format!("Error: {}", error)
                    };
                    lines.push(Line::from(Span::styled(
                        text,
                        Style::default().fg(self.theme.error),
                    )));
                }
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}
