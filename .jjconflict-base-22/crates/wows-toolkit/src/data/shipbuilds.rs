use std::sync::Arc;

#[derive(Clone)]
pub struct ShipBuildsClient {
    http: Arc<reqwest::blocking::Client>,
}

impl ShipBuildsClient {
    pub fn new(proxy: Option<&crate::util::proxy::ProxyConfig>) -> Result<Self, reqwest::Error> {
        // Upload and match-stats callers classify 3xx responses explicitly.
        crate::util::http::blocking_client(reqwest::redirect::Policy::none(), proxy)
            .map(|http| Self { http: Arc::new(http) })
    }

    pub fn http(&self) -> &reqwest::blocking::Client {
        &self.http
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_one_http_client() {
        let original = ShipBuildsClient::new(None).expect("test HTTP client");
        let clone = original.clone();
        assert!(std::ptr::eq(original.http(), clone.http()));
    }
}
