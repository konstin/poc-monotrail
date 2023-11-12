//! Download and install standalone python builds (PyOxy) from
//! <https://github.com/indygreg/python-build-standalone>

use anyhow::{bail, Context};
use fs2::FileExt;
use fs_err as fs;
use fs_err::File;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tempfile::tempdir_in;
use tracing::{debug, info, warn};

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
/// Since https://github.com/indygreg/python-build-standalone/issues/138
/// we ship this with the binary.
/// zstd because it's 2.44% of the original json size.
/// Source: https://github.com/indygreg/python-build-standalone/releases/tag/20220502
const PYTHON_STANDALONE_KNOWN_GOOD_RELEASE: &[u8] =
    include_bytes!("../../../resources/python_build_standalone_known_good_release.json.zst");

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
fn find_python(host: &str, major: u8, minor: u8) -> anyhow::Result<String> {
    let version_re = filename_regex(major, minor);

    let latest_release: anyhow::Result<GitHubRelease> =
        ureq::get(&format!("{}{}", host, PYTHON_STANDALONE_LATEST_RELEASE.0))
            .set("User-Agent", "monotrail (konstin@mailbox.org)")
            .call()
            .map_err(anyhow::Error::new)
            .and_then(|x| x.into_json().map_err(anyhow::Error::new));

    match latest_release {
        Ok(latest_release) => {
            let asset = latest_release.assets.into_iter().find(|asset| {
                // TODO: Proper name parsing
                // https://github.com/indygreg/python-build-standalone/issues/127
                version_re.is_match(&asset.name)
            });
            if let Some(asset) = asset {
                return Ok(asset.browser_download_url);
            }
        }
        Err(err) => {
            warn!(
                "Failed to call github api for latest standalone python release: {}",
                err
            );
        }
    }

    get_known_good_release(major, minor)
}

fn get_known_good_release(major: u8, minor: u8) -> anyhow::Result<String> {
    let version_re = filename_regex(major, minor);

    // unwrap because we know the content
    let good_release: GitHubRelease =
        serde_json::from_slice(&zstd::decode_all(PYTHON_STANDALONE_KNOWN_GOOD_RELEASE).unwrap())
            .unwrap();

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
                "Failed to find a matching python-build-standalone download: /{}/. \
                Searched in {} and https://github.com/indygreg/python-build-standalone/releases/tag/20220502",
                version_re,
                PYTHON_STANDALONE_LATEST_RELEASE.1,
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
    fs::create_dir_all(target_dir)?;
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
        install_dir.join("lib").join("libpython3.so")
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
    let url = find_python(GITHUB_API, python_version.0, python_version.1).with_context(|| {
        format!(
            "Couldn't find a matching python {}.{} to download",
            python_version.0, python_version.1,
        )
    })?;
    // atomic installation by tempdir & rename
    let temp_dir = tempdir_in(python_parent_dir)
        .context("Failed to create temporary directory for unpacking")?;
    match download_and_unpack_python(&url, temp_dir.path()) {
        Ok(()) => {}
        Err(err) => {
            warn!(
                "Failed to download and unpack latest python-build-standalone from {}, \
                using known good release instead. Error: {}",
                url, err
            );
            let url =
                get_known_good_release(python_version.0, python_version.1).with_context(|| {
                    format!(
                        "Couldn't find a matching python {}.{} to download",
                        python_version.0, python_version.1,
                    )
                })?;
            download_and_unpack_python(&url, temp_dir.path())
                .context("Failed to download and unpack python-build-standalone")?;
        }
    }
    // we can use fs::rename here because we stay in the same directory
    fs::rename(temp_dir, unpack_dir).context("Failed to move installed python into place")?;
    debug!("Installed python {}.{}", python_version.0, python_version.1);
    Ok(())
}

/// Returns `(python_binary, python_home)`
pub fn provision_python(
    python_version: (u8, u8),
    cache_dir: &Path,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let python_parent_dir = cache_dir.join("python-build-standalone");
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
    let python_home = unpack_dir.join("python").join("install");
    Ok((python_binary, python_home))
}

/// Returns a regex matching a compatible optimized build from the indygreg/python-build-standalone
/// release page.
///
/// <https://python-build-standalone.readthedocs.io/en/latest/running.html>
pub fn filename_regex(major: u8, minor: u8) -> Regex {
    let target_triple = target_lexicon::HOST.to_string();
    // https://python-build-standalone.readthedocs.io/en/latest/running.html#obtaining-distributions
    let (target_triple, linker_opts) = if target_triple.starts_with("x86_64-unknown-linux") {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
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
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        {
            unreachable!()
        }
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
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    use crate::standalone_python::provision_python;
    use crate::standalone_python::{find_python, PYTHON_STANDALONE_LATEST_RELEASE};
    use mockito::{Mock, ServerGuard};
    use std::path::PathBuf;
    use tempfile::tempdir;

    pub fn zstd_json_mock(url: &str, fixture: impl Into<PathBuf>) -> (ServerGuard, Mock) {
        use fs_err::File;

        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", url)
            .with_header("content-type", "application/json")
            .with_body(zstd::stream::decode_all(File::open(fixture).unwrap()).unwrap())
            .create();
        (server, mock)
    }

    fn mock() -> (ServerGuard, Mock) {
        zstd_json_mock(
            PYTHON_STANDALONE_LATEST_RELEASE.0,
            "../../test-data/standalone_python_github_release.json.zstd",
        )
    }

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_download_url_from_release_20220502() {
        let (server, _mocks) = mock();

        let url = find_python(&server.url(), 3, 9).unwrap();
        assert_eq!(url, "https://github.com/indygreg/python-build-standalone/releases/download/20220502/cpython-3.9.12%2B20220502-x86_64_v3-unknown-linux-gnu-pgo%2Blto-full.tar.zst")
    }

    #[test]
    fn test_download_url_from_release_20220502_any() {
        let (server, _mocks) = mock();

        assert!(find_python(&server.url(), 3, 9).is_ok());
    }

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_provision_nonexistent_version() {
        let _mocks = mock();
        let tempdir = tempdir().unwrap();
        let err = provision_python((3, 0), tempdir.path()).unwrap_err();
        let expected = vec![
            r"Couldn't find a matching python 3.0 to download",
            r"Failed to find a matching python-build-standalone download: /^cpython-3\.0\.(\d+)\+(\d+)-x86_64\-unknown\-linux\-gnu-pgo\+lto-full\.tar\.zst$/. Searched in https://github.com/indygreg/python-build-standalone/releases/latest and https://github.com/indygreg/python-build-standalone/releases/tag/20220502",
        ];
        let actual = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
