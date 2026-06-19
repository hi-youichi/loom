//! Skill tools module.
//!
//! Provides the read-only skill tools used by both the main conversation and
//! the background review path:
//!
//! - [`list::SkillListTool`] (tool name `skill_list`): lists all available
//!   skills with names, descriptions, and categories.
//! - [`view::SkillViewTool`] (tool name `skill_view`): loads a skill's full
//!   content by name, with optional `file_path` to read a sub-file within
//!   the skill directory.
//!
//! Write operations are in [`manage::SkillManagerTool`] (`skill_manage`).
//!
//! ## Construction
//!
//! Use [`make_skill_tools_with_registry`] when you have an `Arc<SkillRegistry>`
//! in hand (the typical case for background review), or
//! [`make_skill_tools_with_folder`] when you only have a working folder path
//! and want tools that walk the directory tree.

use std::sync::Arc;

use skill::SkillRegistry;
use skill::SkillUsageStore;

mod list;
pub mod manage;
mod view;

pub use list::SkillListTool;
pub use manage::{SkillManagerTool, TOOL_SKILL_MANAGE};
pub use view::SkillViewTool;

const SKILLS_SUBDIR: &str = ".loom/skills";

pub(crate) struct SkillContext {
    pub registry: Option<Arc<SkillRegistry>>,
    pub working_folder: Option<Arc<std::path::PathBuf>>,
    pub usage_store: Option<SkillUsageStore>,
}

impl SkillContext {
    pub(crate) fn from_registry(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry: Some(registry),
            working_folder: None,
            usage_store: None,
        }
    }

    pub(crate) fn from_working_folder(working_folder: Arc<std::path::PathBuf>) -> Self {
        let store = {
            let skills_dir = working_folder.join(SKILLS_SUBDIR);
            SkillUsageStore::new(&skills_dir)
        };
        Self {
            registry: None,
            working_folder: Some(working_folder),
            usage_store: Some(store),
        }
    }

    pub(crate) fn with_store(mut self, store: SkillUsageStore) -> Self {
        self.usage_store = Some(store);
        self
    }

    pub(crate) fn skills_dir(&self) -> Option<std::path::PathBuf> {
        self.working_folder
            .as_ref()
            .map(|wf| wf.join(SKILLS_SUBDIR))
    }
}

pub fn make_skill_tools_with_registry(
    registry: Arc<SkillRegistry>,
    usage: Option<SkillUsageStore>,
) -> (SkillListTool, SkillViewTool) {
    let mut ctx = SkillContext::from_registry(registry);
    if let Some(store) = usage {
        ctx = ctx.with_store(store);
    }
    let ctx = Arc::new(ctx);
    (SkillListTool::new(ctx.clone()), SkillViewTool::new(ctx.clone()))
}

pub fn make_skill_tools_with_folder(
    working_folder: Arc<std::path::PathBuf>,
    usage: Option<SkillUsageStore>,
) -> (SkillListTool, SkillViewTool) {
    let mut ctx = SkillContext::from_working_folder(working_folder);
    if let Some(store) = usage {
        ctx = ctx.with_store(store);
    }
    let ctx = Arc::new(ctx);
    (SkillListTool::new(ctx.clone()), SkillViewTool::new(ctx.clone()))
}
