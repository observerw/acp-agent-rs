use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use tokio::fs;
use tokio::task;
use zip::ZipArchive;

use crate::registry::BinaryTarget;

/// Prepared binary distribution ready to run or install.
#[derive(Debug)]
pub struct PreparedBinaryTarget {
    /// Resolved executable path within the extracted payload.
    pub executable_path: PathBuf,
    /// Directory containing the extracted payload.
    pub extracted_dir: PathBuf,
    /// Temporary directory that owns the extracted payload lifetime.
    pub temp_dir: tempfile::TempDir,
}

/// Downloads, extracts, validates, and marks executable the current binary target.
pub async fn prepare_binary_target(target: &BinaryTarget) -> Result<PreparedBinaryTarget> {
    let temp_dir = task::spawn_blocking(tempfile::tempdir)
        .await
        .context("blocking task failed while creating temporary directory")?
        .context("failed to create temporary directory")?;
    let archive_path = download_archive(target, temp_dir.path()).await?;
    let extracted_dir = temp_dir.path().join("extracted");
    fs::create_dir_all(&extracted_dir)
        .await
        .with_context(|| format!("failed to create {}", extracted_dir.display()))?;
    extract_archive(archive_path, extracted_dir.clone()).await?;

    let executable_path = resolve_cmd_path(&extracted_dir, &target.cmd);
    let metadata = fs::metadata(&executable_path).await;
    if metadata
        .as_ref()
        .map(|metadata| !metadata.is_file())
        .unwrap_or(true)
    {
        bail!(
            "downloaded {}, but could not find \"{}\" at {}",
            target.archive,
            target.cmd,
            executable_path.display()
        );
    }

    make_executable(&executable_path)
        .await
        .with_context(|| format!("failed to mark {} executable", executable_path.display()))?;

    Ok(PreparedBinaryTarget {
        executable_path,
        extracted_dir,
        temp_dir,
    })
}

pub(crate) async fn download_archive(target: &BinaryTarget, temp_dir: &Path) -> Result<PathBuf> {
    let url = reqwest::Url::parse(&target.archive)
        .with_context(|| format!("invalid archive URL: {}", target.archive))?;
    let archive_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("download.bin");
    let destination = temp_dir.join(archive_name);

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("failed to download archive from {}", target.archive))?;
    let response = response
        .error_for_status()
        .with_context(|| format!("failed to download archive from {}", target.archive))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read archive response from {}", target.archive))?;
    fs::write(&destination, bytes.as_ref())
        .await
        .with_context(|| {
            format!(
                "failed to write downloaded archive to {}",
                destination.display()
            )
        })?;
    Ok(destination)
}

pub(crate) async fn extract_archive(archive_path: PathBuf, destination: PathBuf) -> Result<()> {
    task::spawn_blocking(move || extract_archive_blocking(&archive_path, &destination))
        .await
        .context("blocking task failed while extracting archive")?
}

fn extract_archive_blocking(archive_path: &Path, destination: &Path) -> Result<()> {
    let file_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if file_name.ends_with(".zip") {
        return extract_zip(archive_path, destination);
    }

    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let file = File::open(archive_path)
            .with_context(|| format!("failed to open archive {}", archive_path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(destination)
            .with_context(|| format!("failed to unpack archive into {}", destination.display()))?;
        return Ok(());
    }

    if file_name.ends_with(".tar.bz2") || file_name.ends_with(".tbz2") {
        let file = File::open(archive_path)
            .with_context(|| format!("failed to open archive {}", archive_path.display()))?;
        let decoder = BzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(destination)
            .with_context(|| format!("failed to unpack archive into {}", destination.display()))?;
        return Ok(());
    }

    let file_name = archive_path
        .file_name()
        .ok_or_else(|| anyhow!("unsupported archive format for {}", archive_path.display()))?;
    let fallback_path = destination.join(file_name);
    std::fs::copy(archive_path, &fallback_path).with_context(|| {
        format!(
            "failed to copy archive {} to {}",
            archive_path.display(),
            fallback_path.display()
        )
    })?;
    Ok(())
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("failed to open archive {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read ZIP archive {}", archive_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read ZIP entry {index}"))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("archive contains unsafe path: {}", entry.name()))?;
        let output_path = destination.join(enclosed);

        if entry.name().ends_with('/') {
            std::fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut output = File::create(&output_path)
            .with_context(|| format!("failed to create {}", output_path.display()))?;
        io::copy(&mut entry, &mut output)
            .with_context(|| format!("failed to extract {}", output_path.display()))?;
    }

    Ok(())
}

pub(crate) fn resolve_cmd_path(extracted_dir: &Path, cmd: &str) -> PathBuf {
    let sanitized = cmd.trim_start_matches("./");
    let candidate = PathBuf::from(sanitized);
    if candidate.is_absolute() {
        return candidate;
    }

    let mut resolved = extracted_dir.to_path_buf();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

pub(crate) async fn make_executable(path: &Path) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path).await?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).await?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}
