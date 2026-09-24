use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{detail_from_body, Error, Result};
use crate::models::*;

pub const DEFAULT_BASE_URL: &str = "https://api.fxrender.com/v1";

// NOTE: FXRender returns `X-RateLimit-Limit` / `X-RateLimit-Remaining` /
// `X-RateLimit-Reset` on every response (see `public_api/docs/API.md`).
// Not surfaced yet — every method here discards headers and returns just
// the decoded body. Worth adding if the CLI ever needs to show/backoff on
// these proactively rather than just handling the 429 `Retry-After`.

/// Rust client for the FXRender public API. One `Client` per token — rate
/// limits are per-token, not per-account.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl Client {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(Error::MissingToken);
        }
        Ok(Self {
            http: reqwest::Client::builder()
                // Cloudflare (or similar) 301s `/jobs` → `/jobs/`. reqwest's
                // default policy then retries that as GET, so create-job
                // receives the job *list* and dies with missing field `id`.
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    match attempt.status().as_u16() {
                        301 | 302 | 303 => attempt.stop(),
                        307 | 308 if attempt.previous().len() < 10 => attempt.follow(),
                        _ => attempt.stop(),
                    }
                }))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn send<T: for<'de> serde::Deserialize<'de>>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T> {
        let resp = req.bearer_auth(&self.token).send().await?;
        let status = resp.status();
        let url = resp.url().to_string();
        let body = resp.text().await?;
        if !status.is_success() {
            eprintln!("[fxrender-core] {} -> {}: {}", url, status.as_u16(), body);
            return Err(detail_from_body(status.as_u16(), &body));
        }
        if body.trim().is_empty() {
            // 204-style no-content responses; caller should use a Result<()>
            // signature and never actually try to decode this branch.
            return Err(Error::Other("empty response body".into()));
        }
        serde_json::from_str(&body).map_err(|e| {
            Error::Other(format!("invalid response body from {url}: {e}"))
        })
    }

    async fn send_unit(&self, req: reqwest::RequestBuilder) -> Result<()> {
        let resp = req.bearer_auth(&self.token).send().await?;
        let status = resp.status();
        let url = resp.url().to_string();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[fxrender-core] {} -> {}: {}", url, status.as_u16(), body);
        Err(detail_from_body(status.as_u16(), &body))
    }

    fn query_pairs(page: i64, page_size: i64) -> [(&'static str, String); 2] {
        [("page", page.to_string()), ("page_size", page_size.to_string())]
    }

    // ---- account ---------------------------------------------------------

    pub async fn me(&self) -> Result<Me> {
        // Production registers `/v1/me` only. A trailing slash 404s
        // (`redirect_slashes=False` and no duplicate `/` route). `/jobs`
        // is the opposite: nginx 301s the no-slash form.
        self.send(self.http.get(self.url("/me"))).await
    }

    pub async fn usage(&self, page: i64, page_size: i64) -> Result<UsageLedger> {
        self.send(
            self.http
                .get(self.url("/usage"))
                .query(&Self::query_pairs(page, page_size)),
        )
        .await
    }

    // ---- assets ------------------------------------------------------------

    pub async fn upload_url(&self, req: &UploadUrlRequest) -> Result<UploadUrlResponse> {
        self.send(self.http.post(self.url("/assets/upload-url")).json(req))
            .await
    }

    /// PUT raw file bytes straight to storage — this call does **not** go
    /// through the FXRender API and carries no bearer token, matching the
    /// signed-URL upload handshake documented in `API.md`.
    pub async fn put_upload(&self, upload_url: &str, content_type: &str, bytes: Vec<u8>) -> Result<()> {
        let resp = self
            .http
            .put(upload_url)
            .header("Content-Type", content_type)
            .body(bytes)
            .send()
            .await?;
        if resp.status().is_success() {
            return Ok(());
        }
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Err(Error::Api { status, detail: body })
    }

    /// Fetch bytes from a short-lived signed `download_url` (e.g. from
    /// [`Client::job_files`]) — no bearer token needed, matching the same
    /// direct-to-storage pattern as [`Client::put_upload`].
    pub async fn download_url(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).send().await?;
        if resp.status().is_success() {
            return Ok(resp.bytes().await?.to_vec());
        }
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Err(Error::Api { status, detail: body })
    }

    pub async fn register_asset(&self, req: &AssetCompleteRequest) -> Result<Asset> {
        self.send(self.http.post(self.url("/assets/")).json(req)).await
    }

    pub async fn list_assets(&self, page: i64, page_size: i64) -> Result<AssetList> {
        self.send(
            self.http
                .get(self.url("/assets/"))
                .query(&Self::query_pairs(page, page_size)),
        )
        .await
    }

    /// Look up an asset you already uploaded by content hash (hex-encoded
    /// SHA-256), so a fresh upload attempt can skip re-uploading identical
    /// bytes. Finds ephemeral matches too, not just kept ones.
    pub async fn find_asset_by_hash(&self, sha256_hex: &str) -> Result<Option<Asset>> {
        let list: AssetList = self
            .send(self.http.get(self.url("/assets/")).query(&[
                ("sha256", sha256_hex),
                ("page", "1"),
                ("page_size", "1"),
            ]))
            .await?;
        Ok(list.items.into_iter().next())
    }

    pub async fn get_asset(&self, asset_id: Uuid) -> Result<Asset> {
        self.send(self.http.get(self.url(&format!("/assets/{asset_id}"))))
            .await
    }

    pub async fn asset_jobs(&self, asset_id: Uuid) -> Result<JobList> {
        self.send(self.http.get(self.url(&format!("/assets/{asset_id}/jobs"))))
            .await
    }

    pub async fn delete_asset(&self, asset_id: Uuid) -> Result<()> {
        self.send_unit(self.http.delete(self.url(&format!("/assets/{asset_id}"))))
            .await
    }

    /// Full upload handshake, but skips the actual upload entirely if this
    /// exact file (by SHA-256) is already known on the server: signed URL
    /// -> PUT bytes -> register, or just a hash lookup if it's a repeat.
    /// `keep_in_library`: FXRender uploads via the API are ephemeral by
    /// default (not listed, deleted after a grace period once any render
    /// job using them finishes) — pass true to keep this one instead. Has
    /// no effect when an existing asset is reused (its own setting stands).
    pub async fn upload_asset(
        &self,
        filename: &str,
        content_type: &str,
        bytes: Vec<u8>,
        keep_in_library: bool,
    ) -> Result<UploadedAsset> {
        let hash = sha256_hex(&bytes);
        if let Some(existing) = self.find_asset_by_hash(&hash).await? {
            return Ok(UploadedAsset { asset: existing, reused: true });
        }

        let size_bytes = bytes.len() as u64;
        let signed = self
            .upload_url(&UploadUrlRequest {
                filename: filename.to_string(),
                content_type: content_type.to_string(),
                size_bytes,
            })
            .await?;
        self.put_upload(&signed.upload_url, content_type, bytes).await?;
        let asset = self.register_asset(&AssetCompleteRequest {
            key: signed.key,
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            size_bytes,
            keep_in_library,
        })
        .await?;
        Ok(UploadedAsset { asset, reused: false })
    }

    // ---- jobs --------------------------------------------------------------

    pub async fn create_job(&self, req: &JobCreate) -> Result<Job> {
        self.send(self.http.post(self.url("/jobs/")).json(req)).await
    }

    pub async fn list_jobs(&self, page: i64, page_size: i64, status: Option<&str>) -> Result<JobList> {
        let mut req = self
            .http
            .get(self.url("/jobs/"))
            .query(&Self::query_pairs(page, page_size));
        if let Some(status) = status {
            req = req.query(&[("status", status)]);
        }
        self.send(req).await
    }

    pub async fn get_job(&self, job_id: Uuid) -> Result<JobDetail> {
        self.send(self.http.get(self.url(&format!("/jobs/{job_id}"))))
            .await
    }

    pub async fn cancel_job(&self, job_id: Uuid) -> Result<Job> {
        self.send(self.http.post(self.url(&format!("/jobs/{job_id}/cancel"))))
            .await
    }

    pub async fn job_files(&self, job_id: Uuid) -> Result<Vec<JobFile>> {
        self.send(self.http.get(self.url(&format!("/jobs/{job_id}/files"))))
            .await
    }

    pub async fn job_movie(&self, job_id: Uuid, req: &MovieRequest) -> Result<Conversion> {
        self.send(self.http.post(self.url(&format!("/jobs/{job_id}/movie"))).json(req))
            .await
    }

    pub async fn job_logs(&self, job_id: Uuid, n: u32, frame: Option<i64>) -> Result<Vec<LogLine>> {
        let mut req = self
            .http
            .get(self.url(&format!("/jobs/{job_id}/logs")))
            .query(&[("n", n.to_string())]);
        if let Some(frame) = frame {
            req = req.query(&[("frame", frame.to_string())]);
        }
        let out: LogsResponse = self.send(req).await?;
        Ok(out.items)
    }

    /// Live tail via Server-Sent Events, for a `--follow` mode. Yields a
    /// `LogEvent::Backlog` once on connect, then one `LogEvent::Line` per
    /// new line as the job renders. Ends when the server closes the
    /// connection (job reached a terminal status) or on a network error.
    pub async fn stream_job_logs(
        &self,
        job_id: Uuid,
    ) -> Result<impl Stream<Item = Result<LogEvent>>> {
        let resp = self
            .http
            .get(self.url(&format!("/jobs/{job_id}/logs/stream")))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(detail_from_body(status.as_u16(), &body));
        }
        Ok(sse_events(resp.bytes_stream()))
    }

    pub async fn estimate(&self, req: &EstimateRequest) -> Result<EstimateResponse> {
        self.send(self.http.post(self.url("/estimate/")).json(req)).await
    }
}

/// Result of [`Client::upload_asset`] — tells the caller whether the bytes
/// were actually sent, so a CLI/GUI can say "reusing an existing upload"
/// instead of implying a fresh one just happened.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UploadedAsset {
    pub asset: Asset,
    pub reused: bool,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone)]
pub enum LogEvent {
    Backlog(Vec<LogLine>),
    Line(LogLine),
    /// A `: keepalive` comment or unrecognized event — safe to ignore.
    Other,
}

/// Turns a raw SSE byte stream (`event: NAME\ndata: JSON\n\n` blocks) into
/// typed [`LogEvent`]s. Minimal by design — no reconnect/retry logic; a CLI's
/// `--follow` loop is expected to just re-call `stream_job_logs` if it drops.
fn sse_events(
    mut bytes: impl Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Unpin,
) -> impl Stream<Item = Result<LogEvent>> {
    async_stream::stream! {
        let mut buf = String::new();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(Error::from)?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find("\n\n") {
                let block: String = buf.drain(..pos + 2).collect();
                if let Some(event) = parse_sse_block(&block) {
                    yield Ok(event);
                }
            }
        }
    }
}

fn parse_sse_block(block: &str) -> Option<LogEvent> {
    let mut event_name = "message".to_string();
    let mut data = String::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim());
        }
    }
    if data.is_empty() {
        return Some(LogEvent::Other);
    }
    match event_name.as_str() {
        "backlog" => serde_json::from_str::<Vec<LogLine>>(&data).ok().map(LogEvent::Backlog),
        "line" => serde_json::from_str::<LogLine>(&data).ok().map(LogEvent::Line),
        _ => Some(LogEvent::Other),
    }
}
