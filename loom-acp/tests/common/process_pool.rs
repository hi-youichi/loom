use super::acp_child::{AcpChild, MockAcpServer};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OnceCell, Semaphore};

const SHORT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

struct PooledProcess {
    acp: std::sync::Mutex<AcpChild>,
    mock: tokio::sync::Mutex<MockAcpServer>,
    initialized: std::sync::Mutex<bool>,
}

impl PooledProcess {
    fn is_alive(&self) -> bool {
        self.acp.lock().unwrap().is_alive()
    }

    async fn respawn(&self) {
        let (new_acp, new_mock) = AcpChild::spawn_with_mock()
            .await
            .expect("respawn loom-acp");
        *self.acp.lock().unwrap() = new_acp;
        *self.mock.lock().await = new_mock;
        *self.initialized.lock().unwrap() = false;
    }

    async fn ensure_initialized(&self) {
        if !*self.initialized.lock().unwrap() {
            self.mock.lock().await.mount_default_responses().await;
            self.acp.lock().unwrap()
                .send_request_and_wait(
                    "initialize",
                    serde_json::json!({ "protocolVersion": 1 }),
                    SHORT_TIMEOUT,
                )
                .await
                .expect("initialize");
            *self.initialized.lock().unwrap() = true;
        }
    }

    async fn drain_by_ping(&self) {
        self.acp.lock().unwrap()
            .send_request_and_wait(
                "session/new",
                serde_json::json!({
                    "cwd": "/tmp/pool-drain",
                    "mcpServers": [],
                }),
                SHORT_TIMEOUT,
            )
            .await
            .expect("drain ping");
    }
}

pub struct AcpProcessPool {
    slots: Vec<Arc<PooledProcess>>,
    semaphore: Arc<Semaphore>,
}

impl AcpProcessPool {
    async fn new(pool_size: usize) -> Self {
        let mut slots = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let (acp, mock) = AcpChild::spawn_with_mock()
                .await
                .expect("spawn pooled loom-acp");
            slots.push(Arc::new(PooledProcess {
                acp: std::sync::Mutex::new(acp),
                mock: tokio::sync::Mutex::new(mock),
                initialized: std::sync::Mutex::new(false),
            }));
        }
        Self {
            slots,
            semaphore: Arc::new(Semaphore::new(pool_size)),
        }
    }

    pub async fn acquire(&self) -> PooledAcpGuard {
        let permit = self.semaphore.clone().acquire_owned().await.expect("semaphore");
        let mut best: Option<Arc<PooledProcess>> = None;
        for slot in &self.slots {
            if slot.is_alive() {
                best = Some(slot.clone());
                break;
            }
        }
        let slot = best.unwrap_or_else(|| self.slots[0].clone());
        if !slot.is_alive() {
            slot.respawn().await;
        }
        slot.ensure_initialized().await;
        slot.mock.lock().await.server.reset().await;
        slot.mock.lock().await.mount_default_responses().await;
        PooledAcpGuard {
            inner: slot,
            _permit: permit,
        }
    }
}

pub struct PooledAcpGuard {
    inner: Arc<PooledProcess>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl PooledAcpGuard {
    pub fn acp(&self) -> std::sync::MutexGuard<'_, AcpChild> {
        self.inner.acp.lock().unwrap()
    }

    pub fn acp_mut(&mut self) -> std::sync::MutexGuard<'_, AcpChild> {
        self.inner.acp.lock().unwrap()
    }

    pub async fn mock(&self) -> tokio::sync::MutexGuard<'_, MockAcpServer> {
        self.inner.mock.lock().await
    }

    pub async fn mock_mut(&mut self) -> tokio::sync::MutexGuard<'_, MockAcpServer> {
        self.inner.mock.lock().await
    }

    pub async fn new_session(&mut self) -> String {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut acp = self.inner.acp.lock().unwrap();
        let request_id = acp.next_request_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "session/new",
            "params": {
                "cwd": cwd.to_str().unwrap_or("."),
                "mcpServers": [],
            }
        });
        let request_str = serde_json::to_string(&request).expect("serialize");
        {
            let mut writer = acp.writer.lock().unwrap();
            writeln!(writer, "{}", request_str).expect("write");
            writer.flush().expect("flush");
        }
        let (_notifications, response) = acp.collect_all_notifications(request_id, DEFAULT_TIMEOUT)
            .expect("collect session/new");
        assert!(
            response.error.is_none(),
            "session/new failed: {:?}",
            response.error
        );
        response
            .result
            .expect("should have result")
            .get("sessionId")
            .and_then(|v| v.as_str())
            .expect("should have sessionId")
            .to_string()
    }
}

static POOL: OnceCell<AcpProcessPool> = OnceCell::const_new();

pub async fn get_pool() -> &'static AcpProcessPool {
    POOL.get_or_init(|| async {
        let pool_size = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4);
        eprintln!("[pool] initializing with {} processes", pool_size);
        AcpProcessPool::new(pool_size).await
    }).await
}
