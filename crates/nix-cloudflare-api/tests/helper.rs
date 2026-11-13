use std::sync::LazyLock;

use nix_cloudflare_sdk::v1::HttpClient;
use tokio::sync::Mutex;

/// Where `wrangler dev` serves the Worker: `NIX_CLOUDFLARE_API_URL`, which tests/run.sh sets for the one it starts, or
/// `wrangler dev`'s own default.
pub fn base_url() -> &'static str {
    static BASE_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("NIX_CLOUDFLARE_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".into())
    });
    &BASE_URL
}

/// The SDK's client, pointed at `wrangler dev`.
pub fn client() -> HttpClient {
    HttpClient::new().with_base_url(base_url())
}

/// The SDK's client, which uploads too: the Worker has no auth.
pub fn uploader() -> HttpClient {
    client()
}

/// Reads a file from `tests/fixture`.
pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixture/{name}")).unwrap()
}

/// Sends a plain GET, for what the typed client doesn't return: the headers.
pub async fn get(path: &str) -> reqwest::Response {
    reqwest::get(format!("{}/{path}", base_url()))
        .await
        .expect("the request failed")
}

/// Sends a plain PUT, for requests the typed client can't make: a content type the spec doesn't declare.
pub async fn put(path: &str, content_type: &str, body: Vec<u8>) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}/{path}", base_url()))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .expect("the request failed")
}

/// Held by every test that sends a `PUT`, so uploads run one at a time.
/// `wrangler dev` drops connections when several arrive at once ("Network
/// connection lost"), and tests that upload the same narinfo would replace
/// what another is about to read back. Reads still run in parallel.
pub static UPLOADS: Mutex<()> = Mutex::const_new(());
