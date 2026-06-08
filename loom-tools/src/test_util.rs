/// Use in any test that sets/removes env vars to prevent data races.
#[cfg(test)]
pub fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// Shared test client for integration tests.
#[cfg(test)]
pub(crate) mod shared_client {
    use std::sync::OnceLock;
    use reqwest::Client;

    static CLIENT: OnceLock<Client> = OnceLock::new();

    pub(crate) fn test_client() -> Client {
        CLIENT
            .get_or_init(|| {
                Client::builder()
                    .tls_built_in_root_certs(false)
                    .no_proxy()
                    .build()
                    .expect("test client")
            })
            .clone()
    }
}