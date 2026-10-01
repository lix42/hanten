//! Where uploads go, and one bounded `POST /v1/events` request.
//!
//! The endpoint is fixed at build time by `NC_TELEMETRY_ENDPOINT` (default: the
//! production Worker, `contracts/telemetry/upload-v1/README.md`). A value is an
//! `https://` URL, an `http://` URL on a loopback host, `none` (this build uploads
//! nothing), or `file:<path>` (append each request body to `<path>` instead of
//! sending it). The same variable at run time may only narrow that, for tests and
//! diagnosis: to `none`, a file or a loopback host. It can never point a build at
//! another remote backend than the one its `enable` notice names, nor revive a
//! `none` build.

use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use super::durable;

/// The production ingestion Worker.
pub const DEFAULT_ENDPOINT: &str = "https://hanten-telemetry.i-70e.workers.dev/v1/events";

/// The whole request — connect, send, response — must finish within this.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest response body read; a real one lists at most 100 IDs.
const RESPONSE_LIMIT: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    None,
    Url(String),
    File(PathBuf),
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Endpoint::None => f.write_str("none"),
            Endpoint::Url(url) => f.write_str(url),
            Endpoint::File(path) => write!(f, "file:{}", path.display()),
        }
    }
}

/// The endpoint this process uploads to. An unparsable value is an error, which
/// callers treat like `none`.
pub fn endpoint() -> Result<Endpoint, String> {
    let built = parse_endpoint(option_env!("NC_TELEMETRY_ENDPOINT").unwrap_or(DEFAULT_ENDPOINT))?;
    match std::env::var("NC_TELEMETRY_ENDPOINT") {
        Ok(runtime) => narrow(built, parse_endpoint(&runtime)?),
        Err(_) => Ok(built),
    }
}

/// The run-time override applied to the build's endpoint.
fn narrow(built: Endpoint, runtime: Endpoint) -> Result<Endpoint, String> {
    let local = match &runtime {
        Endpoint::None | Endpoint::File(_) => true,
        Endpoint::Url(url) => url_is_loopback(url) || runtime == built,
    };
    match built {
        Endpoint::None => Ok(Endpoint::None),
        _ if local => Ok(runtime),
        _ => Err(format!(
            "NC_TELEMETRY_ENDPOINT at run time may only be `none`, `file:<path>` or a \
             loopback URL, not {runtime}"
        )),
    }
}

fn url_is_loopback(url: &str) -> bool {
    url.strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .is_some_and(is_loopback)
}

pub fn parse_endpoint(value: &str) -> Result<Endpoint, String> {
    let bad = |why: &str| format!("NC_TELEMETRY_ENDPOINT {value:?}: {why}");
    if value == "none" {
        return Ok(Endpoint::None);
    }
    if let Some(path) = value.strip_prefix("file:") {
        if path.is_empty() {
            return Err(bad("names no file"));
        }
        return Ok(Endpoint::File(PathBuf::from(path)));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("contains whitespace"));
    }
    if value.contains('@') {
        return Err(bad("a URL with user info is refused"));
    }
    if let Some(rest) = value.strip_prefix("https://") {
        return if rest.is_empty() {
            Err(bad("names no host"))
        } else {
            Ok(Endpoint::Url(value.to_owned()))
        };
    }
    if let Some(rest) = value.strip_prefix("http://") {
        return if is_loopback(rest) {
            Ok(Endpoint::Url(value.to_owned()))
        } else {
            Err(bad("plain http is for a loopback host only"))
        };
    }
    Err(bad(
        "not https://…, http://<loopback>…, file:<path> or none",
    ))
}

/// Whether the authority at the start of `rest` (after the scheme) is loopback.
fn is_loopback(rest: &str) -> bool {
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// A response accounting for a batch, as the Worker sends it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    pub upload_schema_version: u32,
    pub accepted: Vec<String>,
    pub duplicate: Vec<String>,
    pub rejected: Vec<Rejection>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejection {
    pub event_id: String,
    pub code: String,
}

/// What one request came to.
#[derive(Debug)]
pub enum Sent {
    /// HTTP 200 with a parsable acknowledgement (still to be checked against the batch).
    Acknowledged(Acknowledgement),
    /// HTTP 400: the request itself is malformed, so it is never retried.
    Malformed,
    /// Anything else: keep the batch and retry later. `kind` is a fixed label for
    /// `status`, never response text.
    Retry {
        kind: String,
        retry_after_s: Option<u64>,
    },
}

/// Send one request body. Never panics; every failure is a [`Sent::Retry`].
pub fn send(endpoint: &Endpoint, body: &str) -> Sent {
    match endpoint {
        Endpoint::None => Sent::Retry {
            kind: "no_endpoint".into(),
            retry_after_s: None,
        },
        Endpoint::File(path) => match append_body(path, body) {
            Ok(()) => Sent::Acknowledged(acknowledge_all(body)),
            Err(_) => Sent::Retry {
                kind: "io".into(),
                retry_after_s: None,
            },
        },
        Endpoint::Url(url) => post(url, body),
    }
}

fn append_body(path: &std::path::Path, body: &str) -> io::Result<()> {
    let mut file = durable::open_append(path)?;
    let mut line = String::with_capacity(body.len() + 1);
    line.push_str(body);
    line.push('\n');
    file.write_all(line.as_bytes())?;
    file.sync_all()
}

/// A `file:` endpoint accepts everything it is given.
fn acknowledge_all(body: &str) -> Acknowledgement {
    let ids = super::spool::batch_ids(body.as_bytes()).unwrap_or_default();
    Acknowledgement {
        upload_schema_version: super::upload::UPLOAD_SCHEMA_VERSION,
        accepted: ids,
        duplicate: Vec::new(),
        rejected: Vec::new(),
    }
}

fn post(url: &str, body: &str) -> Sent {
    let retry = |kind: &str| Sent::Retry {
        kind: kind.into(),
        retry_after_s: None,
    };
    let loopback = url_is_loopback(url);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent("hanten-telemetry")
        .proxy(if loopback {
            None
        } else {
            ureq::Proxy::try_from_env()
        })
        .build()
        .into();
    let mut response = match agent
        .post(url)
        .header("content-type", "application/json")
        .send(body)
    {
        Ok(r) => r,
        Err(ureq::Error::Timeout(_)) => return retry("timeout"),
        Err(_) => return retry("network"),
    };
    let status = response.status().as_u16();
    let retry_after_s = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    match status {
        200 => {
            let text = response
                .body_mut()
                .with_config()
                .limit(RESPONSE_LIMIT)
                .read_to_string();
            match text.ok().and_then(|t| serde_json::from_str(&t).ok()) {
                Some(ack) => Sent::Acknowledged(ack),
                None => retry("bad_response"),
            }
        }
        400 => Sent::Malformed,
        _ => Sent::Retry {
            kind: format!("http_{status}"),
            retry_after_s,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_values_parse() {
        assert_eq!(parse_endpoint("none"), Ok(Endpoint::None));
        assert_eq!(
            parse_endpoint("file:/tmp/up.jsonl"),
            Ok(Endpoint::File("/tmp/up.jsonl".into()))
        );
        assert_eq!(
            parse_endpoint(DEFAULT_ENDPOINT),
            Ok(Endpoint::Url(DEFAULT_ENDPOINT.into()))
        );
        for ok in [
            "http://127.0.0.1:8080/v1/events",
            "http://localhost/v1/events",
            "http://[::1]:9/v1/events",
        ] {
            assert!(parse_endpoint(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "file:",
            "http://example.com/v1/events",
            "http://127.0.0.1.example.com/v1/events",
            "https://",
            "ftp://x",
            "https://a b",
            "http://127.0.0.1:x@collector.example/v1/events",
            "https://user@hanten-telemetry.example/v1/events",
        ] {
            assert!(parse_endpoint(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_run_time_override_only_narrows() {
        let prod = Endpoint::Url(DEFAULT_ENDPOINT.into());
        let other = Endpoint::Url("https://collector.example/v1/events".into());
        let local = Endpoint::Url("http://127.0.0.1:9/v1/events".into());
        let file = Endpoint::File("/tmp/x".into());
        assert_eq!(narrow(prod.clone(), Endpoint::None), Ok(Endpoint::None));
        assert_eq!(narrow(prod.clone(), file.clone()), Ok(file.clone()));
        assert_eq!(narrow(prod.clone(), local.clone()), Ok(local.clone()));
        assert_eq!(narrow(prod.clone(), prod.clone()), Ok(prod.clone()));
        assert!(narrow(prod.clone(), other.clone()).is_err());
        for runtime in [other, local, file, prod] {
            assert_eq!(narrow(Endpoint::None, runtime), Ok(Endpoint::None));
        }
    }

    #[test]
    fn the_built_in_endpoint_parses() {
        let built = option_env!("NC_TELEMETRY_ENDPOINT").unwrap_or(DEFAULT_ENDPOINT);
        assert!(parse_endpoint(built).is_ok(), "{built}");
    }
}
