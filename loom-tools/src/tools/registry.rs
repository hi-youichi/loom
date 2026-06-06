use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use loom_types::config::BuiltinToolFilter;
use crate::tool_source::{
    load_tool_specs, ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec,
};
use crate::tools::r#trait::Tool;

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    filter: Option<BuiltinToolFilter>,
    dry_run: bool,
    yaml_specs: Option<HashMap<String, ToolSpec>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            filter: None,
            dry_run: false,
            yaml_specs: None,
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn set_filter(&mut self, filter: Option<BuiltinToolFilter>) {
        self.filter = filter;
    }

    pub fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
    }

    pub fn load_yaml_specs(&mut self) -> Result<(), crate::tool_source::YamlSpecError> {
        let specs = load_tool_specs()?;
        let map: HashMap<String, ToolSpec> = specs.into_iter().map(|s| (s.name.clone(), s)).collect();
        self.yaml_specs = Some(map);
        Ok(())
    }

    pub fn apply_yaml_overrides(&self, registered: Vec<ToolSpec>) -> Vec<ToolSpec> {
        match &self.yaml_specs {
            Some(yaml_map) => registered
                .into_iter()
                .map(|r| yaml_map.get(&r.name).cloned().unwrap_or(r))
                .collect(),
            None => registered,
        }
    }

    fn is_allowed(&self, name: &str) -> bool {
        match &self.filter {
            Some(f) => f.is_allowed(name),
            None => true,
        }
    }

    fn raw_specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn list(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self
            .raw_specs()
            .into_iter()
            .filter(|spec| self.is_allowed(&spec.name))
            .collect();
        specs = self.apply_yaml_overrides(specs);
        specs
    }

    pub async fn call(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        if !self.is_allowed(name) {
            return Err(ToolSourceError::NotFound(format!(
                "tool '{}' is disabled for this agent",
                name
            )));
        }
        if self.dry_run {
            return Ok(ToolCallContent::text(format!(
                "(dry run: {} was not executed)",
                name
            )));
        }
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolSourceError::NotFound(name.to_string()))?;
        tool.call(args, ctx).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ToolRegistryLocked {
    inner: Arc<RwLock<ToolRegistry>>,
}

impl ToolRegistryLocked {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolRegistry::new())),
        }
    }

    pub async fn register_async(&self, tool: Box<dyn Tool>) {
        let mut inner = self.inner.write().await;
        inner.register(tool);
    }

    pub fn register_sync(&self, tool: Box<dyn Tool>) {
        let registry = self.inner.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let mut inner = registry.write().await;
                inner.register(tool);
            });
        })
        .join()
        .expect("Failed to join registration thread");
    }

    pub async fn set_filter(&self, filter: Option<BuiltinToolFilter>) {
        let mut inner = self.inner.write().await;
        inner.set_filter(filter);
    }

    pub async fn set_dry_run(&self, dry_run: bool) {
        let mut inner = self.inner.write().await;
        inner.set_dry_run(dry_run);
    }

    pub async fn load_yaml_specs(&self) -> Result<(), crate::tool_source::YamlSpecError> {
        let mut inner = self.inner.write().await;
        inner.load_yaml_specs()
    }

    pub async fn list(&self) -> Vec<ToolSpec> {
        let inner = self.inner.read().await;
        inner.list()
    }

    pub async fn call(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let inner = self.inner.read().await;
        inner.call(name, args, ctx).await
    }
}

impl Default for ToolRegistryLocked {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolSource for ToolRegistryLocked {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        Ok(self.list().await)
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.call(name, arguments, ctx).await
    }
}

#[async_trait]
impl ToolSource for Arc<ToolRegistryLocked> {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        (**self).list_tools().await
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        (**self).call_tool(name, arguments, ctx).await
    }
}
