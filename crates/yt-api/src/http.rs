//! HTTP shim matching sf-api::Http interface (covers, lyrics, images).

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
    pub fn client(&self) -> Result<reqwest::Client, String> {
        Ok(self.client.clone())
    }
    pub fn bypassing(&self, _direct: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.clone()
    }
}

impl Default for Http {
    fn default() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebApiLimits;

impl WebApiLimits {
    pub fn is_limited(&self) -> bool {
        false
    }
    pub fn user_text(&self) -> String {
        String::new()
    }
}
