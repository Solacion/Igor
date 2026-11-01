//! Shared, size- and SHA-256-verified model downloads for Kokoro and Whisper.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub struct ModelFile<'a> {
    pub name: &'a str,
    pub url: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

/// Bound on reading one chunk of a model download; the whole download is not bounded.
const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// A failed speech command, with a message for the operator.
#[derive(Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct SpeechCommandError {
    pub message: String,
}

impl SpeechCommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: u64,
}

pub fn has_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == size)
}

pub fn client() -> Result<reqwest::Client, SpeechCommandError> {
    reqwest::Client::builder()
        .read_timeout(CHUNK_TIMEOUT)
        .build()
        .map_err(|_| SpeechCommandError::new("cannot start the model download"))
}

/// Downloads `file` to `target` through `<target>.part`, renaming only after the size and
/// SHA-256 match. A failed download leaves no `.part` file behind.
pub async fn fetch(
    client: &reqwest::Client,
    file: &ModelFile<'_>,
    target: &Path,
    progress: impl Fn(u64),
) -> Result<(), SpeechCommandError> {
    let part = target.with_file_name(format!("{}.part", file.name));
    let result = fetch_part(client, file, &part, progress).await;
    match result {
        Ok(()) => tokio::fs::rename(&part, target)
            .await
            .map_err(|err| SpeechCommandError::new(format!("cannot save {}: {err}", file.name))),
        Err(err) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(err)
        }
    }
}

async fn fetch_part(
    client: &reqwest::Client,
    file: &ModelFile<'_>,
    part: &Path,
    progress: impl Fn(u64),
) -> Result<(), SpeechCommandError> {
    let failed = |err: &dyn std::fmt::Display| {
        SpeechCommandError::new(format!("downloading {} failed: {err}", file.name))
    };
    let mut response = client
        .get(file.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|err| failed(&err))?;
    let mut out = tokio::fs::File::create(part)
        .await
        .map_err(|err| failed(&err))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|err| failed(&err))? {
        received += chunk.len() as u64;
        if received > file.size {
            return Err(failed(&"the file is larger than expected"));
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await.map_err(|err| failed(&err))?;
        progress(received);
    }
    out.flush().await.map_err(|err| failed(&err))?;
    if received != file.size {
        return Err(failed(&"the file is incomplete"));
    }
    if hex(&hasher.finalize()) != file.sha256 {
        return Err(failed(&"the file does not match its checksum"));
    }
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[tokio::test]
    async fn verifies_then_renames_and_cleans_up_bad_downloads()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("igor-download-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await?;
        for (name, size, checksum, succeeds) in [
            ("valid", 3, hex(&Sha256::digest(b"abc")), true),
            ("checksum", 3, "0".repeat(64), false),
            ("short", 4, hex(&Sha256::digest(b"abc")), false),
            ("large", 2, hex(&Sha256::digest(b"abc")), false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let url = format!("http://{}/model", listener.local_addr()?);
            let server = std::thread::spawn(move || -> std::io::Result<()> {
                let (mut stream, _) = listener.accept()?;
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request)?;
                stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
                )
            });
            // Test fixtures are local; the production URLs and digests are static constants.
            let file = ModelFile {
                name,
                url: &url,
                size,
                sha256: &checksum,
            };
            let target = dir.join(name);
            let result = fetch(&client()?, &file, &target, |_| {}).await;
            assert_eq!(result.is_ok(), succeeds);
            assert_eq!(target.exists(), succeeds);
            assert!(!dir.join(format!("{name}.part")).exists());
            if succeeds {
                assert_eq!(tokio::fs::read(&target).await?, b"abc");
            }
            server
                .join()
                .map_err(|_| "download fixture thread panicked")??;
        }
        tokio::fs::remove_dir_all(dir).await?;
        Ok(())
    }
}
