use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::BuiltinToolFilter;

use crate::{Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

pub struct ArcTool(pub Arc<dyn Tool>);

#[async_trait::async_trait]
impl Tool for ArcTool {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn spec(&self) -> ToolSpec {
        self.0.spec()
    }

    async fn call(
        &self,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.0.call(args, ctx).await
    }
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    filter: Option<BuiltinToolFilter>,
    call_filter: Option<BuiltinToolFilter>,
    dry_run: bool,
    yaml_specs: Option<HashMap<String, ToolSpec>>,
    context: Arc<std::sync::RwLock<Option<ToolCallContext>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            filter: None,
            call_filter: None,
            dry_run: false,
            yaml_specs: None,
            context: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn set_filter(&mut self, filter: Option<BuiltinToolFilter>) {
        self.filter = filter;
    }

    pub fn set_call_filter(&mut self, filter: Option<BuiltinToolFilter>) {
        self.call_filter = filter;
    }

    pub fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
    }

    pub fn load_yaml_specs(&mut self) -> Result<(), crate::YamlSpecError> {
        let specs = crate::load_tool_specs()?;
        let map: HashMap<String, ToolSpec> =
            specs.into_iter().map(|s| (s.name.clone(), s)).collect();
        self.yaml_specs = Some(map);
        Ok(())
    }

    fn apply_yaml_overrides(&self, registered: Vec<ToolSpec>) -> Vec<ToolSpec> {
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

    pub fn list(&self) -> Vec<ToolSpec> {
        let specs: Vec<ToolSpec> = self
            .tools
            .values()
            .map(|tool| tool.spec())
            .filter(|spec| self.is_allowed(&spec.name))
            .collect();
        self.apply_yaml_overrides(specs)
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
        if let Some(cf) = &self.call_filter {
            if !cf.is_allowed(name) {
                return Err(ToolSourceError::NotFound(format!(
                    "tool '{}' is denied by call_filter for this agent",
                    name
                )));
            }
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
    fn set_context(&self, ctx: ToolCallContext) {
        if let Ok(mut g) = self.context.write() {
            *g = Some(ctx);
        }
    }

    fn get_context(&self) -> Option<ToolCallContext> {
        self.context.read().ok().and_then(|g| g.as_ref().cloned())
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

    pub async fn set_call_filter(&self, filter: Option<BuiltinToolFilter>) {
        let mut inner = self.inner.write().await;
        inner.set_call_filter(filter);
    }

    pub async fn set_dry_run(&self, dry_run: bool) {
        let mut inner = self.inner.write().await;
        inner.set_dry_run(dry_run);
    }

    pub async fn load_yaml_specs(&self) -> Result<(), crate::YamlSpecError> {
        let mut inner = self.inner.write().await;
        inner.load_yaml_specs()
    }

    pub async fn list_tools(&self) -> Vec<ToolSpec> {
        let inner = self.inner.read().await;
        inner.list()
    }

    pub async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let inner = self.inner.read().await;
        if let Some(c) = ctx {
            inner.set_context(c.clone());
            inner.call(name, args, ctx).await
        } else {
            let effective_ctx = inner.get_context();
            inner.call(name, args, effective_ctx.as_ref()).await
        }
    }
}

impl Default for ToolRegistryLocked {
    fn default() -> Self {
        Self::new()
    }
}
