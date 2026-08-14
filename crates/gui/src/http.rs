//! HTTP client bridge for GPUI's image loader.
//!
//! GPUI can load images from a URI, but only if the application supplies an
//! `HttpClient`. Rather than pull in a second HTTP stack, this bridges the
//! trait onto reqwest, which the core already depends on.
//!
//! Only what the image loader needs is implemented: `send` for GET requests
//! with in-memory bodies. Streaming request bodies are read fully before
//! dispatch, which is fine for avatar and emoji fetches and avoids a
//! streaming adapter for a case that never occurs here.

use std::sync::Arc;

use futures::AsyncReadExt;
use futures::future::BoxFuture;
use gpui_http_client::{AsyncBody, HttpClient, Inner, http};

/// A `reqwest`-backed client for GPUI's asset loading.
pub struct ReqwestClient {
    inner: reqwest::Client,
    user_agent: http::HeaderValue,
}

impl ReqwestClient {
    pub fn new() -> Arc<dyn HttpClient> {
        // Discord's CDN serves avatars and emoji without authentication, but
        // rejects requests with no user agent.
        let user_agent = concat!("concord-gui/", env!("CARGO_PKG_VERSION"));

        Arc::new(Self {
            inner: reqwest::Client::builder()
                .user_agent(user_agent)
                .build()
                .unwrap_or_default(),
            user_agent: http::HeaderValue::from_static(concat!(
                "concord-gui/",
                env!("CARGO_PKG_VERSION")
            )),
        })
    }
}

/// Read an `AsyncBody` into bytes.
async fn body_bytes(body: AsyncBody) -> Vec<u8> {
    match body.0 {
        Inner::Empty => Vec::new(),
        Inner::Bytes(cursor) => cursor.into_inner().to_vec(),
        Inner::AsyncReader(mut reader) => {
            let mut buffer = Vec::new();
            let _ = reader.read_to_end(&mut buffer).await;
            buffer
        }
    }
}

impl HttpClient for ReqwestClient {
    fn type_name(&self) -> &'static str {
        "reqwest"
    }

    fn user_agent(&self) -> Option<&http::HeaderValue> {
        Some(&self.user_agent)
    }

    /// No proxy support. Avatar fetches go direct; honouring system proxy
    /// settings is a settings-screen concern that does not exist yet.
    fn proxy(&self) -> Option<&gpui_http_client::Url> {
        None
    }

    fn send(
        &self,
        request: http::Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<http::Response<AsyncBody>>> {
        let client = self.inner.clone();
        let (parts, body) = request.into_parts();

        Box::pin(async move {
            let bytes = body_bytes(body).await;

            let mut outgoing = client.request(parts.method, parts.uri.to_string());
            for (name, value) in parts.headers.iter() {
                outgoing = outgoing.header(name, value);
            }
            if !bytes.is_empty() {
                outgoing = outgoing.body(bytes);
            }

            let response = outgoing.send().await?;
            let status = response.status();
            let headers = response.headers().clone();
            let payload = response.bytes().await?;

            let mut builder = http::Response::builder().status(status);
            for (name, value) in headers.iter() {
                builder = builder.header(name, value);
            }

            Ok(builder.body(AsyncBody::from(payload.to_vec()))?)
        })
    }
}
