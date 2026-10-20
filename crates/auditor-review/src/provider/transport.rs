use std::fmt;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::{Client, Request};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{redirect, Method};

pub struct TransportRequest {
    pub url: String,
    pub headers: Vec<TransportHeader>,
    pub body: Vec<u8>,
    pub timeout_ms: u64,
    pub max_response_bytes: usize,
}

pub enum TransportHeader {
    Literal {
        name: String,
        value: String,
    },
    Environment {
        name: String,
        variable: String,
        prefix: String,
    },
}

#[derive(Debug)]
pub struct TransportResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait Transport: Send + Sync {
    fn post(&self, request: TransportRequest) -> Result<TransportResponse, String>;

    fn get(&self, _request: TransportRequest) -> Result<TransportResponse, String> {
        Err("transport does not support model discovery".to_string())
    }
}

#[derive(Default)]
pub struct HttpTransport {
    client: OnceLock<Result<Client, String>>,
}

impl fmt::Debug for HttpTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HttpTransport")
    }
}

impl Transport for HttpTransport {
    fn post(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        self.request(request, Method::POST)
    }

    fn get(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        self.request(request, Method::GET)
    }
}

impl HttpTransport {
    fn request(
        &self,
        request: TransportRequest,
        method: Method,
    ) -> Result<TransportResponse, String> {
        let (request, max_response_bytes) = self.build_request(request, method)?;
        let mut response = self.client()?.execute(request).map_err(|error| {
            let kind = if error.is_timeout() {
                "timed out"
            } else {
                "failed"
            };
            format!("HTTP transport request {kind}: {}", error.without_url())
        })?;
        let status = response.status().as_u16();
        let body = read_bounded_body(&mut response, max_response_bytes)?;
        Ok(TransportResponse { status, body })
    }

    fn build_request(
        &self,
        request: TransportRequest,
        method: Method,
    ) -> Result<(Request, usize), String> {
        if request.max_response_bytes == 0 {
            return Err("HTTP transport response budget must be positive".to_string());
        }
        let max_response_bytes = request.max_response_bytes;
        let client = self.client()?;
        let mut builder = client
            .request(method.clone(), &request.url)
            .timeout(Duration::from_millis(request.timeout_ms));
        for header in request.headers {
            let (name, value) = resolve_header(header)?;
            builder = builder.header(name, value);
        }
        if method == Method::POST {
            builder = builder.body(request.body);
        }
        let request = builder
            .build()
            .map_err(|error| format!("failed to build HTTP request: {}", error.without_url()))?;
        Ok((request, max_response_bytes))
    }

    fn client(&self) -> Result<&Client, String> {
        self.client
            .get_or_init(|| {
                Client::builder()
                    .no_proxy()
                    .redirect(redirect::Policy::none())
                    .build()
                    .map_err(|error| {
                        format!(
                            "failed to initialize HTTP transport: {}",
                            error.without_url()
                        )
                    })
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}

fn read_bounded_body(reader: &mut impl Read, max_response_bytes: usize) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    Read::by_ref(reader)
        .take(max_response_bytes.saturating_add(1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| format!("failed to read HTTP response: {error}"))?;
    if body.len() > max_response_bytes {
        return Err(format!(
            "HTTP response exceeds {max_response_bytes} byte budget"
        ));
    }
    Ok(body)
}

fn resolve_header(header: TransportHeader) -> Result<(HeaderName, HeaderValue), String> {
    let (name, value) = match header {
        TransportHeader::Literal { name, value } => (name, value),
        TransportHeader::Environment {
            name,
            variable,
            prefix,
        } => {
            let secret = std::env::var(&variable).map_err(|_| {
                format!("provider API key environment variable is unset: {variable}")
            })?;
            (name, format!("{prefix}{secret}"))
        }
    };
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| "provider request contains an invalid header name".to_string())?;
    let value = HeaderValue::from_str(&value)
        .map_err(|_| "provider request contains an invalid header value".to_string())?;
    Ok((name, value))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn request(max_response_bytes: usize) -> TransportRequest {
        TransportRequest {
            url: "http://127.0.0.1:8080/v1/responses".to_string(),
            headers: vec![TransportHeader::Literal {
                name: "content-type".to_string(),
                value: "application/json".to_string(),
            }],
            body: br#"{"probe":true}"#.to_vec(),
            timeout_ms: 2_000,
            max_response_bytes,
        }
    }

    #[test]
    fn builds_typed_stateless_request_with_deadline_and_body() {
        let transport = HttpTransport::default();
        let (request, response_budget) =
            transport.build_request(request(64), Method::POST).unwrap();
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.url().as_str(), "http://127.0.0.1:8080/v1/responses");
        assert_eq!(request.headers()["content-type"], "application/json");
        assert_eq!(request.timeout(), Some(&Duration::from_millis(2_000)));
        assert_eq!(
            request.body().and_then(|body| body.as_bytes()),
            Some(br#"{"probe":true}"#.as_slice())
        );
        assert_eq!(response_budget, 64);
        assert!(std::ptr::eq(
            transport.client().unwrap(),
            transport.client().unwrap()
        ));
    }

    #[test]
    fn response_reader_enforces_exact_byte_budget() {
        assert_eq!(
            read_bounded_body(&mut Cursor::new(b"1234"), 4).unwrap(),
            b"1234"
        );
        let error = read_bounded_body(&mut Cursor::new(b"12345"), 4).unwrap_err();
        assert!(error.contains("exceeds 4 byte budget"));
    }

    #[test]
    fn rejects_zero_response_budget_before_network_execution() {
        let error = HttpTransport::default()
            .build_request(request(0), Method::GET)
            .err()
            .unwrap();
        assert!(error.contains("response budget must be positive"));
    }
}
