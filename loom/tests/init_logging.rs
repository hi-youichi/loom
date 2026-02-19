//! Test-only: initializes tracing from `RUST_LOG` when the test binary starts.
//!
//! Include `mod init_logging;` in an integration test file so that tracing events
//! from the loom library (e.g. `tracing::debug!` in `openai.rs`) are printed
//! when running tests. Without this, no subscriber is installed and logs are dropped.
//!
//! **Usage**: run with `RUST_LOG` and show output for (all or failing) tests:
//!
//! ```bash
//! RUST_LOG=loom=debug cargo test -p loom -- --nocapture
//! RUST_LOG=debug cargo test -p loom openai_sse -- --nocapture
//! ```

use ctor::ctor;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

#[ctor]
fn init_test_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_test_writer()
                .with_filter(filter),
        )
        .try_init();
}
