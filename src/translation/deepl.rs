use serde::{Deserialize, Serialize};

use super::{
    TranslationError, TranslationFuture, TranslationProvider, TranslationRequest,
    TranslationResult, parse_endpoint,
};

const PROVIDER: &str = "DeepL";

pub(super) struct DeepLProvider {
    http: reqwest::Client,
    endpoint: reqwest::Url,
    api_key: String,
}

impl DeepLProvider {
    pub(super) fn new(
        http: reqwest::Client,
        endpoint: &str,
        api_key: String,
    ) -> Result<Self, TranslationError> {
        Ok(Self {
            http,
            endpoint: parse_endpoint(PROVIDER, endpoint)?,
            api_key,
        })
    }

    fn request(&self, request: TranslationRequest<'_>) -> reqwest::RequestBuilder {
        self.http
            .post(self.endpoint.clone())
            .header(
                reqwest::header::AUTHORIZATION,
                format!("DeepL-Auth-Key {}", self.api_key),
            )
            .json(&DeepLRequest {
                text: [request.text],
                target_lang: request.target_language.to_ascii_uppercase(),
            })
    }
}

impl TranslationProvider for DeepLProvider {
    fn translate<'a>(&'a self, request: TranslationRequest<'a>) -> TranslationFuture<'a> {
        Box::pin(async move {
            let response =
                self.request(request)
                    .send()
                    .await
                    .map_err(|source| TranslationError::Request {
                        provider: PROVIDER,
                        source,
                    })?;
            if !response.status().is_success() {
                return Err(TranslationError::Http {
                    provider: PROVIDER,
                    status: response.status(),
                });
            }
            let response = response.json::<DeepLResponse>().await.map_err(|source| {
                TranslationError::InvalidResponse {
                    provider: PROVIDER,
                    source,
                }
            })?;
            let translation = response
                .translations
                .into_iter()
                .next()
                .ok_or(TranslationError::EmptyResponse { provider: PROVIDER })?;
            if translation.text.trim().is_empty() {
                return Err(TranslationError::EmptyResponse { provider: PROVIDER });
            }
            Ok(TranslationResult {
                text: translation.text,
            })
        })
    }
}

#[derive(Serialize)]
struct DeepLRequest<'a> {
    text: [&'a str; 1],
    target_lang: String,
}

#[derive(Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}

#[derive(Deserialize)]
struct DeepLTranslation {
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_follow_deepl_contract() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let provider = DeepLProvider::new(
            reqwest::Client::new(),
            "https://api-free.deepl.com/v2/translate",
            "secret".to_owned(),
        )
        .expect("test endpoint is valid");

        let request = provider
            .request(TranslationRequest {
                text: "hello",
                target_language: "ko",
            })
            .build()
            .expect("request should build");

        assert_eq!(
            request.headers()[reqwest::header::AUTHORIZATION],
            "DeepL-Auth-Key secret"
        );
        let body: serde_json::Value = serde_json::from_slice(
            request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .expect("JSON body"),
        )
        .expect("body should be JSON");
        assert_eq!(
            body,
            serde_json::json!({ "text": ["hello"], "target_lang": "KO" })
        );

        let response: DeepLResponse = serde_json::from_value(serde_json::json!({
            "translations": [{ "detected_source_language": "EN", "text": "안녕하세요" }]
        }))
        .expect("response should parse");

        assert_eq!(response.translations[0].text, "안녕하세요");
    }
}
