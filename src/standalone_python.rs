//! Download and install standalone python builds (PyOxy) from
//! <https://github.com/indygreg/python-build-standalone>

use crate::monotrail::{LaunchType, PythonContext};
use crate::utils::cache_dir;
use crate::Pep508Environment;
use anyhow::{bail, Context};
use fs2::FileExt;
use fs_err as fs;
use fs_err::File;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tempfile::tempdir_in;
use tracing::{debug, info};
#[cfg_attr(test, allow(dead_code))]
const GITHUB_API: &str = "https://api.github.com";

const PYTHON_STANDALONE_LATEST_RELEASE: (&str, &str) = (
    // api url
    "/repos/indygreg/python-build-standalone/releases/latest",
    // web url. doesn't help to much showing the api url, i just hope github has their stuff
    // together enough that these always match
    "https://github.com/indygreg/python-build-standalone/releases/latest",
);

/// i've manually confirmed that this release has python 3.8, 3.9 and 3.10 for all major
/// platforms and a naming convention monotrail can read
const PYTHON_STANDALONE_KNOWN_GOOD_RELEASE: (&str, &str) = (
    "/repos/indygreg/python-build-standalone/releases/65881217",
    "https://github.com/indygreg/python-build-standalone/releases/tag/20220502",
);

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
    #[cfg(not(test))]
    let host = GITHUB_API;

    #[cfg(test)]
    let host = &mockito::server_url();

    let latest_release: GitHubRelease =
        ureq::get(&format!("{}{}", host, PYTHON_STANDALONE_LATEST_RELEASE.0))
            .set("User-Agent", "monotrail (konstin@mailbox.org)")
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

    let good_release: GitHubRelease = ureq::get(&format!(
        "{}{}",
        host, PYTHON_STANDALONE_KNOWN_GOOD_RELEASE.0
    ))
    .set("User-Agent", "monotrail (konstin@mailbox.org)")
    .call()?
    .into_json()?;

    let asset = good_release
        .assets
        .into_iter()
        .find(|asset| {
            // TODO: Proper name parsing
            // https://github.com/indygreg/python-build-standalone/issues/127
            version_re.is_match(&asset.name)
        })
        .with_context(|| {
            format!(
                "Failed to find a matching python-build-standalone download: /{}/. Searched in {} and {}", 
                version_re,
                PYTHON_STANDALONE_LATEST_RELEASE.1,
                PYTHON_STANDALONE_KNOWN_GOOD_RELEASE.1,
            )
        })?;
    Ok(asset.browser_download_url)
}

/// Download the prebuilt python .tar.zstd and unpacks it into the the target dir
fn download_and_unpack_python(url: &str, target_dir: &Path) -> anyhow::Result<()> {
    // TODO: Add MB from API
    info!("Downloading {}", url);
    let tar_zstd = ureq::get(url)
        .set("User-Agent", "monotrail (konstin@mailbox.org)")
        .call()?
        .into_reader();
    let tar = zstd::Decoder::new(tar_zstd)?;
    let mut archive = tar::Archive::new(tar);
    fs::create_dir_all(&target_dir)?;
    archive.unpack(target_dir)?;
    Ok(())
}

/// Check whether the installed python looks good or broken
fn check_installed_python(unpack_dir: &Path, python_version: (u8, u8)) -> anyhow::Result<()> {
    let install_dir = unpack_dir.join("python").join("install");
    let lib = if cfg!(target_os = "macos") {
        install_dir.join("lib").join(format!(
            "libpython{}.{}.dylib",
            python_version.0, python_version.1
        ))
    } else if cfg!(target_os = "windows") {
        install_dir.join(format!("python3{}.dll", python_version.1))
    } else {
        // Assume generic unix otherwise (tested for linux)
        install_dir.join("lib").join("libpython3.so".to_string())
    };
    if !lib.is_file() {
        bail!(
            "broken python installation in {}. \
                Try deleting the directory and running again",
            unpack_dir.display()
        )
    }
    // Good installation, reuse
    debug!("Python {}.{} ready", python_version.0, python_version.1);
    Ok(())
}

/// Actual download and move into place logic
fn provision_python_inner(
    python_version: (u8, u8),
    python_parent_dir: &PathBuf,
    unpack_dir: &PathBuf,
) -> anyhow::Result<()> {
    debug!(
        "Installing python {}.{}",
        python_version.0, python_version.1
    );
    let url = find_python(python_version.0, python_version.1).with_context(|| {
        format!(
            "Couldn't find a matching python {}.{} to download",
            python_version.0, python_version.1,
        )
    })?;
    // atomic installation by tempdir & rename
    let temp_dir = tempdir_in(&python_parent_dir)
        .context("Failed to create temporary directory for unpacking")?;
    download_and_unpack_python(&url, temp_dir.path())?;
    // we can use fs::rename here because we stay in the same directory
    fs::rename(temp_dir, &unpack_dir).context("Failed to move installed python into place")?;
    debug!("Installed python {}.{}", python_version.0, python_version.1);
    Ok(())
}

/// If a downloaded python version exists, return this, otherwise download and unpack a matching one
/// from indygreg/python-build-standalone
pub fn provision_python(python_version: (u8, u8)) -> anyhow::Result<(PythonContext, PathBuf)> {
    let python_parent_dir = cache_dir()?.join("python-build-standalone");
    // We need this here for the locking logic
    fs::create_dir_all(&python_parent_dir).context("Failed to create cache dir")?;
    let unpack_dir =
        python_parent_dir.join(format!("cpython-{}.{}", python_version.0, python_version.1));

    if unpack_dir.is_dir() {
        check_installed_python(&unpack_dir, python_version)?;
    } else {
        // If two processes are started in parallel that both install python, the second one will fail
        // because it can't move the installed directory because it already exists. To avoid this, only
        // one process at
        let install_lock = python_parent_dir.join(format!(
            "cpython-{}.{}.install-lock",
            python_version.0, python_version.1
        ));
        let lockfile = File::create(install_lock)?;
        if lockfile.file().try_lock_exclusive().is_ok() {
            provision_python_inner(python_version, &python_parent_dir, &unpack_dir)?;
        } else {
            info!("Waiting for other process to finish installing");
            lockfile.file().lock_exclusive()?;
            // Maybe the other process failed
            let result = if unpack_dir.is_dir() {
                info!("The other process seems to have succeeded");
                // Check if ok install, ok if true, error if not
                check_installed_python(&unpack_dir, python_version)
            } else {
                info!("The other process seems to have failed, installing");
                provision_python_inner(python_version, &python_parent_dir, &unpack_dir)
            };
            // Make sure we unlock the file before returning. This would be nicer if it would
            // work through drop on a file lock object
            lockfile.file().unlock()?;
            result?;
        }
    }

    let python_binary = if cfg!(target_os = "windows") {
        unpack_dir.join("python").join("install").join("python.exe")
    } else {
        // Tested for linux and mac
        unpack_dir
            .join("python")
            .join("install")
            .join("bin")
            .join("python3")
    };
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
/// <https://python-build-standalone.readthedocs.io/en/latest/running.html>
pub fn filename_regex(major: u8, minor: u8) -> Regex {
    let target_triple = target_lexicon::HOST.to_string();
    // https://python-build-standalone.readthedocs.io/en/latest/running.html#obtaining-distributions
    let (target_triple, linker_opts) = if target_triple.starts_with("x86_64-unknown-linux") {
        cpufeatures::new!(cpu_v3, "avx2");
        cpufeatures::new!(cpu_v2, "sse4.2");
        // For python3.8 there's only the base version
        let target_triple = if cpu_v3::init().get() && minor > 8 {
            target_triple.replace("x86_64", "x86_64_v3")
        } else if cpu_v2::init().get() && minor > 8 {
            target_triple.replace("x86_64", "x86_64_v2")
        } else {
            target_triple
        };
        (target_triple, "pgo+lto")
    } else if target_triple.ends_with("pc-windows-msvc") {
        (format!("{}-shared", target_triple), "pgo")
    } else {
        (target_triple, "pgo+lto")
    };

    let version_re = format!(
        r#"^cpython-{major}\.{minor}\.(\d+)\+(\d+)-{target_triple}-{linker_opts}-full\.tar\.zst$"#,
        major = major,
        minor = minor,
        target_triple = regex::escape(&target_triple),
        linker_opts = regex::escape(linker_opts),
    );
    Regex::new(&version_re)
        .context("Failed to build version regex")
        .unwrap()
}

#[cfg(test)]
mod test {
    use mockito::Mock;

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    use crate::standalone_python::provision_python;
    use crate::standalone_python::{
        find_python, PYTHON_STANDALONE_KNOWN_GOOD_RELEASE, PYTHON_STANDALONE_LATEST_RELEASE,
    };
    use crate::utils::zstd_json_mock;

    fn mock() -> (Mock, Mock) {
        let latest_mock = zstd_json_mock(
            PYTHON_STANDALONE_LATEST_RELEASE.0,
            "test-data/standalone_python_github_release.json.zstd",
        );
        let known_good_mock = zstd_json_mock(
            PYTHON_STANDALONE_KNOWN_GOOD_RELEASE.0,
            "test-data/standalone_python_known_good_release.json.zstd",
        );
        (latest_mock, known_good_mock)
    }

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_download_url_from_release_20220502() {
        let _mocks = mock();

        let url = find_python(3, 9).unwrap();
        assert_eq!(url, "https://github.com/indygreg/python-build-standalone/releases/download/20220502/cpython-3.9.12%2B20220502-x86_64_v3-unknown-linux-gnu-pgo%2Blto-full.tar.zst")
    }

    #[test]
    fn test_download_url_from_release_20220502_any() {
        let _mocks = mock();

        assert!(find_python(3, 9).is_ok());
    }

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_provision_nonexistent_version() {
        let _mocks = mock();
        let err = provision_python((3, 0)).unwrap_err();
        let expected = vec![
            r"Couldn't find a matching python 3.0 to download",
            r"Failed to find a matching python-build-standalone download: /^cpython-3\.0\.(\d+)\+(\d+)-x86_64\-unknown\-linux\-gnu-pgo\+lto-full\.tar\.zst$/. Searched in https://github.com/indygreg/python-build-standalone/releases/latest and https://github.com/indygreg/python-build-standalone/releases/tag/20220502",
        ];
        let actual = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
