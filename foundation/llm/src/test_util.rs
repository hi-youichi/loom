#[cfg(test)]
pub(crate) mod shared_client {
    use reqwest::Client;
    use std::sync::OnceLock;

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
