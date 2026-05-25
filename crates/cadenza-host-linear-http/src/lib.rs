//! Production `reqwest`-backed transport for the `host-linear` capability
//! (ADR 0008, issue #84).
//!
//! [`ReqwestLinearTransport`] implements the synchronous
//! [`cadenza_wasm_host::LinearTransport`] the `host-linear` capability consumes.
//! It is the host-side injector of the operator's Linear API token — the token
//! lives in this struct, never in [`LinearCall`], never crosses the WIT into
//! guest memory, and is never logged.
//!
//! Cancellation (the reason this crate exists): every request sets its own
//! deadline from [`LinearCall::timeout`] via `reqwest`'s per-request
//! `.timeout(..)`. When it fires during the send / await-response phase — where
//! a `GraphqlMode::Write` mutation actually reaches the server — reqwest aborts
//! the request at the transport layer (the HTTP/1.1 connection is dropped or the
//! HTTP/2 stream is reset), so a timed-out write is no longer silently left in
//! flight (ADR 0007 "Known limitations", resolved here). Exactly-once for writes
//! is still not a client guarantee; non-idempotent mutations need caller-side
//! idempotency.
//!
//! Scope of the deadline (be precise, do not overclaim): reqwest's blocking
//! client applies `call.timeout` *per `read`* on the response body rather than
//! as one total budget. The send / await-response phase — the security-relevant
//! one for write cancellation — is bounded by `call.timeout`. A trickling body,
//! however, can hold the detached worker past `call.timeout`; that occupancy is
//! bounded instead by [`LinearCall::max_response_bytes`] (total bytes) and the
//! process-wide in-flight worker ceiling (ADR 0007 Decision 5, which fails closed
//! with `rate-limited`). The endpoint is host-configured and allowlist-checked,
//! not guest-chosen, so a trickle requires a compromised configured upstream and
//! still degrades fail-closed.
//!
//! HTTP/2 is negotiated via ALPN (the `http2` cargo feature is re-enabled here
//! because the workspace pin sets `default-features = false`), falling back to
//! HTTP/1.1; `http2_prior_knowledge` is deliberately not forced.

use std::io::Read;

use cadenza_wasm_host::{LinearCall, LinearHttpResult, LinearTransport, LinearTransportError};
use reqwest::blocking::{Client, Response};

/// Synchronous reqwest transport for `host-linear`. Holds the operator's Linear
/// API token; construct one per operator credential and hand it to a
/// `LinearCapability`.
pub struct ReqwestLinearTransport {
    token: String,
    client: Client,
}

// Manual `Debug` so the operator token can never reach a log line or panic
// message through a `{:?}` on the transport — or on any struct that embeds it
// and derives `Debug` (ADR 0006). The `LinearTransport` trait requires `Debug`;
// this satisfies it without ever materialising the credential.
impl std::fmt::Debug for ReqwestLinearTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestLinearTransport")
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl ReqwestLinearTransport {
    /// Build a transport bound to `token`. The client carries no global timeout
    /// — each call's deadline comes from [`LinearCall::timeout`] and is applied
    /// per-request, so a single client serves calls with differing deadlines.
    ///
    /// Like [`LinearTransport::execute`], this uses `reqwest::blocking` and so
    /// must not be called from within an async runtime (the production wiring
    /// constructs it on the host, off any runtime).
    pub fn new(token: impl Into<String>) -> Result<Self, reqwest::Error> {
        let client = Client::builder().build()?;
        Ok(Self {
            token: token.into(),
            client,
        })
    }
}

impl LinearTransport for ReqwestLinearTransport {
    fn execute(&self, call: LinearCall) -> Result<LinearHttpResult, LinearTransportError> {
        // `variables_json` is already valid JSON (the capability validates and
        // normalises it before constructing the call).
        let variables: serde_json::Value = serde_json::from_str(&call.variables_json)
            .map_err(|e| LinearTransportError::Io(format!("invalid variables json: {e}")))?;
        let mut body = serde_json::Map::new();
        body.insert(
            "query".to_string(),
            serde_json::Value::String(call.query.clone()),
        );
        body.insert("variables".to_string(), variables);
        if let Some(op) = &call.operation_name {
            body.insert(
                "operationName".to_string(),
                serde_json::Value::String(op.clone()),
            );
        }
        let body = serde_json::Value::Object(body);

        // `.timeout(call.timeout)` is the cancellation mechanism: reqwest aborts
        // the request (drops the connection / resets the HTTP/2 stream) when the
        // deadline elapses, so the detached worker is reclaimed promptly instead
        // of blocking on a hung upstream (ADR 0008). `.bearer_auth` injects the
        // operator token host-side; the guest has no header channel. The exact
        // auth scheme mirrors `cadenza_tracker_linear`'s transport and is
        // confirmed at production-wiring time.
        let resp = self
            .client
            .post(&call.endpoint)
            .bearer_auth(&self.token)
            .json(&body)
            .timeout(call.timeout)
            .send()
            .map_err(map_reqwest_error)?;

        let status = resp.status();
        // 429: surface the typed rate-limit variant with the parsed `Retry-After`
        // so the capability/orchestrator can branch on it. Only the integer-
        // seconds form is parsed; the HTTP-date form degrades to `None` ("no
        // hint"), matching `RateLimited(Option<u32>)`'s seconds-or-nothing shape.
        if status.as_u16() == 429 {
            let hint = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u32>().ok());
            return Err(LinearTransportError::RateLimited(hint));
        }
        // Other non-2xx is a transport/upstream failure. Keep the detail generic
        // (status only — never the URL, token, or response body); the capability
        // scrubs and caps it again before the guest sees anything.
        if !status.is_success() {
            return Err(LinearTransportError::Upstream(format!(
                "HTTP {} from linear endpoint",
                status.as_u16()
            )));
        }
        // Read the body bounded to `max_response_bytes` so an oversized upstream
        // response cannot force a large host allocation before the capability's
        // backstop length check runs.
        let body_json = read_body_capped(resp, call.max_response_bytes)?;
        Ok(LinearHttpResult {
            status: status.as_u16(),
            body_json,
        })
    }
}

/// Read at most `max` bytes of the response body, failing closed if the body
/// exceeds the cap rather than materialising an unbounded allocation.
fn read_body_capped(resp: Response, max: usize) -> Result<String, LinearTransportError> {
    // `reqwest::blocking::Response` implements `std::io::Read`. Read one byte
    // past the cap so an exactly-`max` body is accepted while anything larger is
    // detected without reading it all.
    let mut buf = Vec::new();
    resp.take(max as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| LinearTransportError::Io(format!("read response body: {e}")))?;
    if buf.len() > max {
        return Err(LinearTransportError::Upstream(
            "linear response body too large".to_string(),
        ));
    }
    String::from_utf8(buf)
        .map_err(|_| LinearTransportError::Io("response body is not valid utf-8".to_string()))
}

/// Map a reqwest failure to the typed transport error. Messages are static and
/// carry neither the URL nor the token. A timeout is the cancellation path: the
/// host watchdog (ADR 0007) is the authority for `host-error::timeout`, so by
/// the time this returns the host has typically already surfaced the timeout —
/// the point of the transport's own deadline is that it *aborts the request* and
/// frees the worker.
fn map_reqwest_error(err: reqwest::Error) -> LinearTransportError {
    if err.is_timeout() {
        LinearTransportError::Io("linear transport request timed out".to_string())
    } else {
        LinearTransportError::Io("linear transport request failed".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn parse_content_length(headers: &[u8]) -> usize {
        let text = String::from_utf8_lossy(headers).to_ascii_lowercase();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("content-length:") {
                if let Ok(n) = rest.trim().parse::<usize>() {
                    return n;
                }
            }
        }
        0
    }

    /// Read a full HTTP/1.1 request (headers + `Content-Length` body) so the
    /// client's write completes and it is left awaiting our response.
    fn read_request(stream: &mut TcpStream) -> std::io::Result<()> {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 2048];
        loop {
            let n = stream.read(&mut tmp)?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                let content_len = parse_content_length(&buf[..pos]);
                let mut have = buf.len() - (pos + 4);
                while have < content_len {
                    let n = stream.read(&mut tmp)?;
                    if n == 0 {
                        break;
                    }
                    have += n;
                }
                return Ok(());
            }
        }
    }

    /// Bind a one-shot server on loopback and run `handler` against the single
    /// accepted connection on a background thread. Returns the `http://` URL.
    fn serve_once<F>(handler: F) -> String
    where
        F: FnOnce(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let url = format!("http://{}/graphql", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handler(stream);
            }
        });
        url
    }

    fn call(endpoint: &str, timeout: Duration, max_response_bytes: usize) -> LinearCall {
        LinearCall {
            operation_name: Some("Q".to_string()),
            query: "query Q { viewer { id } }".to_string(),
            variables_json: "{}".to_string(),
            mode: cadenza_wasm_host::LinearMode::Read,
            endpoint: endpoint.to_string(),
            max_response_bytes,
            timeout,
        }
    }

    // ADR 0006: the operator token must never reach a log or panic. A `{:?}` on
    // the transport must not render it. Guards the manual `Debug` impl — drop it
    // (fall back to `#[derive(Debug)]`) and this fails.
    #[test]
    fn debug_redacts_the_operator_token() {
        let transport = ReqwestLinearTransport::new("lr_live_SUPERSECRET").unwrap();
        let rendered = format!("{transport:?}");
        assert!(
            !rendered.contains("lr_live_SUPERSECRET"),
            "token leaked through Debug: {rendered}",
        );
        assert!(rendered.contains("redacted"), "got {rendered}");
    }

    #[test]
    fn ok_response_returns_status_and_body() {
        let url = serve_once(|mut stream| {
            read_request(&mut stream).ok();
            let body = r#"{"data":{"viewer":{"id":"u_1"}}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).ok();
        });
        let transport = ReqwestLinearTransport::new("lr_live_HOSTONLY").unwrap();
        let res = transport
            .execute(call(&url, Duration::from_secs(5), 1024))
            .expect("ok response");
        assert_eq!(res.status, 200);
        assert!(res.body_json.contains("u_1"));
    }

    // AC4: a request aborted at the client timeout surfaces the typed error AND
    // the server records no completion (it never sent a response). The server
    // reads the request then sleeps 3s without responding; the client's 150ms
    // timeout must fire first, abort the request, and return well under a 1s
    // race-free bound. `responded` is flipped true only immediately before a
    // write that never happens, so asserting it stays false proves no upstream
    // completion crossed back. Mutation: drop `.timeout(call.timeout)` and the
    // client blocks until the server drops the connection at ~3s, breaking the
    // sub-1s bound.
    #[test]
    fn timed_out_request_is_aborted_with_no_upstream_completion() {
        let responded = Arc::new(AtomicBool::new(false));
        let server_flag = Arc::clone(&responded);
        let url = serve_once(move |mut stream| {
            read_request(&mut stream).ok();
            std::thread::sleep(Duration::from_secs(3));
            // Past the client's deadline — the client has already aborted. A real
            // upstream "completion" would set this before writing; we never do.
            server_flag.store(true, Ordering::SeqCst);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .ok();
        });
        let transport = ReqwestLinearTransport::new("lr_live_HOSTONLY").unwrap();
        let started = Instant::now();
        let err = transport
            .execute(call(&url, Duration::from_millis(150), 1024))
            .expect_err("a timed-out request must surface an error");
        let elapsed = started.elapsed();

        assert!(matches!(err, LinearTransportError::Io(_)), "got {err:?}");
        assert!(
            elapsed < Duration::from_secs(1),
            "client did not abort at its own timeout (blocked for {elapsed:?})",
        );
        // No upstream completion crossed back before/at the abort.
        assert!(
            !responded.load(Ordering::SeqCst),
            "transport observed an upstream completion after the timeout",
        );
    }

    #[test]
    fn rate_limited_status_maps_to_typed_variant_with_retry_hint() {
        let url = serve_once(|mut stream| {
            read_request(&mut stream).ok();
            stream
                .write_all(
                    b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 30\r\nContent-Length: 0\r\n\r\n",
                )
                .ok();
        });
        let transport = ReqwestLinearTransport::new("lr_live_HOSTONLY").unwrap();
        let err = transport
            .execute(call(&url, Duration::from_secs(5), 1024))
            .expect_err("429 must surface");
        assert!(
            matches!(err, LinearTransportError::RateLimited(Some(30))),
            "got {err:?}",
        );
    }

    #[test]
    fn server_error_maps_to_upstream() {
        let url = serve_once(|mut stream| {
            read_request(&mut stream).ok();
            stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
                .ok();
        });
        let transport = ReqwestLinearTransport::new("lr_live_HOSTONLY").unwrap();
        let err = transport
            .execute(call(&url, Duration::from_secs(5), 1024))
            .expect_err("500 must surface");
        assert!(
            matches!(err, LinearTransportError::Upstream(_)),
            "got {err:?}"
        );
    }

    // The transport must bound its own read: an over-cap body is rejected, not
    // returned, so it cannot force a large host allocation.
    #[test]
    fn oversized_response_body_is_rejected() {
        let url = serve_once(|mut stream| {
            read_request(&mut stream).ok();
            let body = "x".repeat(64);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).ok();
        });
        let transport = ReqwestLinearTransport::new("lr_live_HOSTONLY").unwrap();
        let err = transport
            .execute(call(&url, Duration::from_secs(5), 16))
            .expect_err("oversized body must be rejected");
        assert!(
            matches!(err, LinearTransportError::Upstream(ref m) if m.contains("too large")),
            "got {err:?}",
        );
    }
}
