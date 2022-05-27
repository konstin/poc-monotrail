use crate::monotrail::{LaunchType, PythonContext};
use crate::Pep508Environment;
use anyhow::{bail, Context};
use fs_err as fs;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tempfile::tempdir_in;
use tracing::info;

const PYTHON_STANDALONE_LATEST_RELEASE: &str =
    "https://api.github.com/repos/indygreg/python-build-standalone/releases/latest";

const PYTHON_STANDALONE_KNOWN_GOOD_RELEASE: &str =
    "https://api.github.com/repos/indygreg/python-build-standalone/releases/65881217";

#[derive(Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Returns the url of the matching pgo+lto prebuilt python. We first try to find one in the latest
/// indygreg/python-build-standalone, then fall back to a known good release in case a more recent
/// release broke compatibility
fn find_python(major: u8, minor: u8) -> anyhow::Result<String> {
    let latest_release: GitHubRelease = ureq::get(PYTHON_STANDALONE_LATEST_RELEASE)
        .call()?
        .into_json()?;

    let version_re = filename_regex(major, minor);
    let asset = latest_release.assets.into_iter().find(|asset| {
        // TODO: Proper name parsing
        // https://github.com/indygreg/python-build-standalone/issues/127
        version_re.is_match(&asset.name)
    });
    if let Some(asset) = asset {
        return Ok(asset.browser_download_url);
    }

    let latest_release: GitHubRelease = ureq::get(PYTHON_STANDALONE_KNOWN_GOOD_RELEASE)
        .call()?
        .into_json()?;

    let asset = latest_release
        .assets
        .into_iter()
        .find(|asset| {
            // TODO: Proper name parsing
            // https://github.com/indygreg/python-build-standalone/issues/127
            version_re.is_match(&asset.name)
        })
        .with_context(|| {
            format!(
                "Failed to find a matching python-build-standalone download: {}",
                version_re
            )
        })?;
    Ok(asset.browser_download_url)
}

/// Download the prebuilt python .tar.zstd and unpacks it into the the target dir
fn download_and_unpack_python(url: &str, target_dir: &Path) -> anyhow::Result<()> {
    // TODO: Add MB from API
    info!("Downloading {}", url);
    let tar_zstd = ureq::get(url).call()?.into_reader();
    let tar = zstd::Decoder::new(tar_zstd)?;
    let mut archive = tar::Archive::new(tar);
    fs::create_dir_all(&target_dir)?;
    archive.unpack(target_dir)?;
    Ok(())
}

/// If a downloaded python version exists, return this, otherwise download and unpack a matching one
/// from indygreg/python-build-standalone
pub fn provision_python(python_version: (u8, u8)) -> anyhow::Result<(PythonContext, PathBuf)> {
    // TODO: use monotrail mechanism
    let python_parent_dir = dirs::cache_dir()
        .context("Cache dir not found")?
        .join("monotrail")
        .join("python-build-standalone");
    fs::create_dir_all(&python_parent_dir).context("Failed to create cache dir")?;
    let unpack_dir =
        python_parent_dir.join(format!("cpython-{}.{}", python_version.0, python_version.1));

    if unpack_dir.is_dir() {
        let lib_dir = unpack_dir.join("python").join("install").join("lib");
        let lib = if cfg!(target_os = "macos") {
            format!("libpython{}.{}.dylib", python_version.0, python_version.1)
        } else {
            "libpython3.so".to_string()
        };
        if !lib_dir.join(lib).is_file() {
            bail!("broken python installation in {}", unpack_dir.display())
        }
        // Good installation, reuse
    } else {
        let url = find_python(python_version.0, python_version.1)?;
        // atomic installation by tempdir & rename
        let temp_dir = tempdir_in(&python_parent_dir)
            .context("Failed to create temporary directory for unpacking")?;
        download_and_unpack_python(&url, temp_dir.path())?;
        // we can use fs::rename here because we stay in the same directory
        fs::rename(temp_dir, &unpack_dir)?;
    }
    let python_binary = unpack_dir
        .join("python")
        .join("install")
        .join("bin")
        .join("python3");
    // TODO: Already init and use libpython here
    let pep508_env = Pep508Environment::from_python(&python_binary);
    let python_context = PythonContext {
        sys_executable: python_binary,
        version: python_version,
        pep508_env,
        launch_type: LaunchType::Binary,
    };

    let python_home = unpack_dir.join("python").join("install");
    Ok((python_context, python_home))
}

/// Returns a regex matching a compatible optimized build from the indygreg/python-build-standalone
/// release page.
///
/// https://python-build-standalone.readthedocs.io/en/latest/running.html
pub fn filename_regex(major: u8, minor: u8) -> Regex {
    let target_triple = target_lexicon::HOST.to_string();
    let target_triple = if target_triple.starts_with("x86_64-unknown-linux") {
        cpufeatures::new!(cpu_v3, "avx2");
        cpufeatures::new!(cpu_v2, "sse4.2");
        // For python3.8 there's only the base version
        if cpu_v3::init().get() && minor > 8 {
            target_triple.replace("x86_64", "x86_64_v3")
        } else if cpu_v2::init().get() && minor > 8 {
            target_triple.replace("x86_64", "x86_64_v2")
        } else {
            target_triple
        }
    } else {
        target_triple
    };

    let version_re = format!(
        r#"^cpython-{major}\.{minor}\.(\d+)\+(\d+)-{target_triple}-pgo\+lto-full\.tar\.zst$"#,
        major = major,
        minor = minor,
        target_triple = regex::escape(&target_triple)
    );
    Regex::new(&version_re)
        .context("Failed to build version regex")
        .unwrap()
}

#[cfg(test)]
mod test {
    use crate::standalone_python::{filename_regex, GitHubRelease};
    use std::fs;

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_download_url_from_release_20220502() {
        let data_20220502: GitHubRelease = serde_json::from_str(
            &fs::read_to_string("test-data/standalone_python_github_release.json").unwrap(),
        )
        .unwrap();
        let version_re = filename_regex(3, 9);
        let asset = data_20220502
            .assets
            .into_iter()
            .find(|asset| version_re.is_match(&asset.name))
            .unwrap();
        assert_eq!(asset.browser_download_url, "https://github.com/indygreg/python-build-standalone/releases/download/20220502/cpython-3.9.12%2B20220502-x86_64_v3-unknown-linux-gnu-pgo%2Blto-full.tar.zst")
    }

    #[test]
    fn test_download_url_from_release_20220502_any() {
        let data_20220502: GitHubRelease = serde_json::from_str(
            &fs::read_to_string("test-data/standalone_python_github_release.json").unwrap(),
        )
        .unwrap();
        let version_re = filename_regex(3, 9);
        data_20220502
            .assets
            .into_iter()
            .find(|asset| version_re.is_match(&asset.name))
            .unwrap();
    }
}
