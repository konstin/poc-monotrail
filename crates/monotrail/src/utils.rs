use crate::cli::{run_cli, Cli};
use fs_err as fs;
use fs_err::DirEntry;
use install_wheel_rs::Error;
#[cfg(test)]
use mockito::{Mock, ServerGuard};
use std::io;
use std::path::{Path, PathBuf};

/// Return all subdirs in a directory
pub fn get_dir_content(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let read_dir = fs::read_dir(Path::new(&dir))?;
    Ok(read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect())
}

/// `~/.cache/monotrail`
pub(crate) fn cache_dir() -> Result<PathBuf, Error> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| {
            Error::IO(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a cache dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}

/// `~/.local/share/monotrail`
pub(crate) fn data_local_dir() -> Result<PathBuf, Error> {
    Ok(dirs::data_local_dir()
        .ok_or_else(|| {
            Error::IO(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a data dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}

/// This is used by several places for testing
#[doc(hidden)]
pub fn assert_cli_error(cli: Cli, venv: Option<&Path>, expected: &[&str]) {
    if let Err(err) = run_cli(cli, venv) {
        // Convert windows path to unix paths
        let actual = err
            .chain()
            .map(|e| e.to_string().replace('\\', "/"))
            .collect::<Vec<_>>();
        assert_eq!(expected, actual);
    } else {
        panic!("Should have errored");
    }
}

/// Adds the mock response for a prerecorded .json.zstd response
#[cfg(test)]
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
