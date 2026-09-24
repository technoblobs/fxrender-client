use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{status}: {detail}")]
    Api { status: u16, detail: String },

    #[error("invalid response body: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("no API token configured — run `fxr login`")]
    MissingToken,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// FXRender's error shape is always `{"detail": ...}` — a string for most
/// errors, or a list of `{"loc","msg","type"}` objects for a 422 validation
/// failure. Flatten both into one display-friendly string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DetailBody {
    Message { detail: String },
    Validation { detail: Vec<ValidationItem> },
    Unknown(serde_json::Value),
}

#[derive(Debug, Deserialize)]
struct ValidationItem {
    #[serde(default)]
    loc: Vec<serde_json::Value>,
    #[serde(default)]
    msg: String,
}

pub(crate) fn detail_from_body(status: u16, body: &str) -> Error {
    let detail = match serde_json::from_str::<DetailBody>(body) {
        Ok(DetailBody::Message { detail }) => detail,
        Ok(DetailBody::Validation { detail }) => detail
            .into_iter()
            .map(|item| {
                let loc = item
                    .loc
                    .iter()
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .collect::<Vec<_>>()
                    .join(".");
                if loc.is_empty() {
                    item.msg
                } else {
                    format!("{loc}: {}", item.msg)
                }
            })
            .collect::<Vec<_>>()
            .join("; "),
        Ok(DetailBody::Unknown(v)) => v.to_string(),
        Err(_) if body.trim().is_empty() => format!("HTTP {status}"),
        Err(_) => body.to_string(),
    };
    Error::Api { status, detail }
}
