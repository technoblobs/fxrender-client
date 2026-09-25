//! Blender versions the public API accepts: 4.2, 4.5, 5.0, and 5.3-alpha.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Turn a user string into an API `blender_version`.
///
/// Accepts `4.2`, `4.5`, `5.0`, `5.3`, `5.3-alpha`, and a leading `blender-`.
pub fn normalize_blender_version(raw: &str) -> Result<String, String> {
    let s = raw.trim().to_ascii_lowercase();
    let s = s.strip_prefix("blender-").unwrap_or(&s);
    let s = s.strip_prefix("blender").unwrap_or(s).trim_start_matches(['-', ' ']);
    match s {
        "4.2" | "4.2lts" | "4.2-lts" => Ok("4.2".into()),
        "4.5" | "4.5lts" | "4.5-lts" => Ok("4.5".into()),
        "5.0" | "5" => Ok("5.0".into()),
        "5.3" | "5.3-alpha" | "5.3.0" | "5.3.0-alpha" | "5.3alpha" | "5.3_alpha" => {
            Ok("5.3-alpha".into())
        }
        "auto" => Ok("auto".into()),
        other => Err(format!(
            "unknown Blender version `{other}` — use 4.2, 4.5, 5.0, 5.3-alpha, or auto"
        )),
    }
}

/// Pick a farm version from an inspect `blender_saved` string. Unknown values
/// stay on 4.5, which is what the API uses when a client omits the field.
pub fn blender_version_from_saved(saved: Option<&str>) -> String {
    let s = saved.unwrap_or("").trim().to_ascii_lowercase();
    if s.contains("5.3") {
        "5.3-alpha".into()
    } else if s.starts_with("5.0") || s == "5" {
        "5.0".into()
    } else if s.contains("4.2") {
        "4.2".into()
    } else {
        "4.5".into()
    }
}

/// Read the version written in a `.blend` header. Compressed files and
/// unrecognized headers return `None` (the caller keeps its own default).
pub fn sniff_blend_file(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0u8; 16];
    let n = file.read(&mut buf).ok()?;
    version_from_header(&buf[..n]).map(str::to_string)
}

pub fn version_from_header(buf: &[u8]) -> Option<&'static str> {
    if buf.len() < 12 || &buf[..7] != b"BLENDER" {
        return None;
    }
    // Classic 12-byte header: BLENDER, pointer size, endian, 3-digit version.
    let code = if matches!(buf[7], b'-' | b'_') && matches!(buf[8], b'v' | b'V') {
        &buf[9..12]
    // Blender 5 header: BLENDER, 2-digit file-format version, pointer, endian, version.
    } else if buf.len() >= 14
        && buf[7].is_ascii_digit()
        && buf[8].is_ascii_digit()
        && matches!(buf[9], b'-' | b'_')
        && matches!(buf[10], b'v' | b'V')
    {
        &buf[11..14]
    } else {
        return None;
    };
    match code {
        b"402" => Some("4.2"),
        b"405" | b"450" => Some("4.5"),
        b"500" => Some("5.0"),
        b"503" => Some("5.3-alpha"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases() {
        assert_eq!(normalize_blender_version("5.3").unwrap(), "5.3-alpha");
        assert_eq!(normalize_blender_version("blender-5.3-alpha").unwrap(), "5.3-alpha");
        assert_eq!(normalize_blender_version("4.5").unwrap(), "4.5");
        assert_eq!(normalize_blender_version("5.0").unwrap(), "5.0");
        assert!(normalize_blender_version("2.93").is_err());
    }

    #[test]
    fn classic_and_v5_headers() {
        let mut classic = *b"BLENDER-v405....";
        assert_eq!(version_from_header(&classic), Some("4.5"));
        classic[9..12].copy_from_slice(b"503");
        // pointer/endian still classic; version digits overwritten above only if layout matches.
        let mut file = *b"BLENDER-v503....";
        assert_eq!(version_from_header(&file), Some("5.3-alpha"));
        file = *b"BLENDER17-v503..";
        assert_eq!(version_from_header(&file), Some("5.3-alpha"));
        let _ = classic;
    }

    #[test]
    fn saved_string() {
        assert_eq!(blender_version_from_saved(Some("5.3.0 Alpha")), "5.3-alpha");
        assert_eq!(blender_version_from_saved(Some("4.2")), "4.2");
        assert_eq!(blender_version_from_saved(None), "4.5");
    }
}
