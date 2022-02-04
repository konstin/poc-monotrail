use configparser::ini::Ini;
use fs_err as fs;
use fs_err::{DirEntry, File};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, io};
use thiserror::Error;
use tracing::{debug, span, trace, warn, Level};
use walkdir::WalkDir;
use zip::result::ZipError;
use zip::ZipArchive;

#[derive(Error, Debug)]
pub enum WheelInstallerError {
    #[error(transparent)]
    IOError(#[from] io::Error),
    /// This shouldn't actually be possible to occur
    #[error("Failed to serialize direct_url.json ಠ_ಠ")]
    DirectUrlSerdeJsonError(#[source] serde_json::Error),
    /// The wheel is broken
    #[error("The wheel is invalid: {0}")]
    InvalidWheel(String),
    /// The wheel is broken, but in python pkginfo
    #[error("The wheel is broken")]
    PkgInfoError(#[from] python_pkginfo::Error),
    #[error("Failed to read the wheel file")]
    ZipError(#[from] ZipError),
    #[error("Failed to run python subcommand")]
    PythonSubcommandError(#[source] io::Error),
    #[error("Failed to move data files")]
    WalkDirError(#[source] walkdir::Error),
    #[error("RECORD file doesn't match wheel contents: {0}")]
    RecordFileError(String),
    #[error("RECORD file is invalid")]
    RecordCsvError(#[from] csv::Error),
    #[error("Broken virtualenv: {0}")]
    BrokenVenv(String),
}

/// Line in a RECORD file
/// https://www.python.org/dev/peps/pep-0376/#record
///
/// ```csv
/// tqdm/cli.py,sha256=x_c8nmc4Huc-lKEsAXj78ZiyqSJ9hJ71j7vltY67icw,10509
/// tqdm-4.62.3.dist-info/RECORD,,
/// ```
#[derive(Deserialize, Serialize, PartialOrd, PartialEq, Ord, Eq)]
struct RecordEntry {
    path: String,
    hash: Option<String>,
    #[allow(dead_code)]
    size: Option<usize>,
}

/// Minimal direct_url.json schema
///
/// https://packaging.python.org/en/latest/specifications/direct-url/
/// https://www.python.org/dev/peps/pep-0610/
#[derive(Serialize)]
struct DirectUrl {
    archive_info: HashMap<(), ()>,
    url: String,
}

/// Wrapper script template function
///
/// https://github.com/pypa/pip/blob/7f8a6844037fb7255cfd0d34ff8e8cf44f2598d4/src/pip/_vendor/distlib/scripts.py#L41-L48
fn get_script_launcher(module: &str, import_name: &str, python: &Path) -> String {
    format!(
        r##"#!{python}
# -*- coding: utf-8 -*-
import re
import sys
from {module} import {import_name}
if __name__ == '__main__':
    sys.argv[0] = re.sub(r'(-script\.pyw|\.exe)?$', '', sys.argv[0])
    sys.exit({import_name}())
"##,
        python = python.display(),
        module = module,
        import_name = import_name
    )
}

/// Parses the entry_points.txt entry in the wheel for console scripts
fn parse_console_scripts(
    archive: &mut ZipArchive<File>,
    dist_info_dir: &str,
) -> Result<Vec<(String, String, String)>, WheelInstallerError> {
    let entry_points_path = format!("{}/entry_points.txt", dist_info_dir);
    let entry_points_mapping = match archive.by_name(&entry_points_path) {
        Ok(mut file) => {
            let mut ini_text = String::new();
            file.read_to_string(&mut ini_text)?;
            Ini::new().read(ini_text).map_err(|err| {
                WheelInstallerError::InvalidWheel(format!("entry_points.txt is invalid: {}", err))
            })?
        }
        Err(ZipError::FileNotFound) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    // TODO: handle extras
    let console_scripts_section = match entry_points_mapping.get("console_scripts") {
        Some(console_scripts) => console_scripts,
        None => return Ok(Vec::new()),
    };

    let mut console_scripts = Vec::new();
    for (script_name, python_location) in console_scripts_section.iter() {
        match python_location {
            Some(value) => {
                if value.contains(' ') {
                    return Err(WheelInstallerError::InvalidWheel(
                        "Extras in console scripts aren't supported yet".to_string(),
                    ));
                }
                let (module, function) = value.split_once(":").ok_or_else(|| {
                    WheelInstallerError::InvalidWheel(format!(
                        "console_scripts is invalid: console script key {} must contain a colon",
                        script_name
                    ))
                })?;
                console_scripts.push((
                    script_name.to_string(),
                    module.to_string(),
                    function.to_string(),
                ));
            }
            None => {
                return Err(WheelInstallerError::InvalidWheel(format!(
                    "[console_script] key {} must have a value",
                    script_name
                )));
            }
        }
    }

    Ok(console_scripts)
}

/// Shamelessly stolen (and updated for recent sha2)
/// https://github.com/richo/hashing-copy/blob/d8dd2fdb63c6faf198de0c9e5713d6249cbb5323/src/lib.rs#L10-L52
/// which in turn got it from std
/// https://doc.rust-lang.org/1.58.0/src/std/io/copy.rs.html#128-156

pub fn copy_and_hash(reader: &mut impl Read, writer: &mut impl Write) -> io::Result<(u64, String)> {
    // TODO: Do we need to support anything besides sha256?
    let mut hasher = Sha256::new();
    // Same buf size as std. Note that this number is important for performance
    let mut buf = vec![0; 8 * 1024];

    let mut written = 0;
    loop {
        let len = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(len) => len,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buf[..len]);
        writer.write_all(&buf[..len])?;
        written += len as u64;
    }
    Ok((
        written,
        format!(
            "sha256={}",
            base64::encode_config(&hasher.finalize(), base64::URL_SAFE_NO_PAD)
        ),
    ))
}

/// Extract all files from the wheel into the site packages
///
/// Matches with the RECORD entries
///
/// Returns paths relative to site packages
fn unpack_wheel_files(
    site_packages: &Path,
    record_path: &str,
    archive: &mut ZipArchive<File>,
    record: &[RecordEntry],
) -> Result<Vec<PathBuf>, WheelInstallerError> {
    let mut extracted_paths = Vec::new();
    // https://github.com/zip-rs/zip/blob/7edf2489d5cff8b80f02ee6fc5febf3efd0a9442/examples/extract.rs
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        // enclosed_name takes care of evil zip paths
        let relative = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };
        let out_path = site_packages.join(&relative);

        if (&*file.name()).ends_with('/') {
            // pip seems to do ignore those folders, so do we
            // fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(p) = out_path.parent() {
            if !p.exists() {
                fs::create_dir_all(&p)?;
            }
        }
        let mut outfile = BufWriter::new(File::create(&out_path)?);
        let (_size, encoded_hash) = copy_and_hash(&mut file, &mut outfile)?;

        extracted_paths.push(relative.clone());

        // Get and Set permissions
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&out_path, Permissions::from_mode(mode))?;
            }
        }

        // This is the RECORD file that contains the hashes so naturally it can't contain it's own
        // hash and size (but it does contain an entry with two empty fields)
        // > 6. RECORD.jws is used for digital signatures. It is not mentioned in RECORD.
        // > 7. RECORD.p7s is allowed as a courtesy to anyone who would prefer to use S/MIME
        // >    signatures to secure their wheel files. It is not mentioned in RECORD.
        let record_path = PathBuf::from(&record_path);
        if vec![
            record_path.clone(),
            record_path.with_extension("jws"),
            record_path.with_extension("p7s"),
        ]
        .contains(&relative)
        {
            continue;
        }

        // `relative == Path::new(entry.path)` was really slow
        let relative_str = relative.display().to_string();
        let recorded_hash = record
            .iter()
            .find(|entry| relative_str == entry.path)
            .and_then(|entry| entry.hash.as_ref())
            .ok_or_else(|| {
                WheelInstallerError::RecordFileError(format!(
                    "Missing hash for {}",
                    relative.display()
                ))
            })?;
        if recorded_hash != &encoded_hash {
            return Err(WheelInstallerError::RecordFileError(format!(
                "Hash mismatch for {}. Recorded: {}, Actual: {}",
                relative.display(),
                recorded_hash,
                encoded_hash,
            )));
        }
    }
    Ok(extracted_paths)
}

/// Create the wrapper scripts in the bin folder of the venv for launching console scripts
///
/// We also pass venv_base so we can write the same path as pip does
fn write_entrypoints(
    site_packages: &Path,
    venv_base: &Path,
    entrypoints: Vec<(String, String, String)>,
    record: &mut Vec<RecordEntry>,
) -> Result<(), WheelInstallerError> {
    for entrypoint in entrypoints {
        let entrypoint_relative = Path::new("../../../bin").join(entrypoint.0);
        let launcher_python_script = get_script_launcher(
            &entrypoint.1,
            &entrypoint.2,
            // canonicalize on python would resolve the symlink
            &venv_base.canonicalize()?.join("bin").join("python"),
        );
        write_file_recorded(
            site_packages,
            &entrypoint_relative,
            &launcher_python_script,
            record,
        )?;
        // We need to make the launcher executable
        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                site_packages.join(entrypoint_relative),
                std::fs::Permissions::from_mode(0o755),
            )?;
        }
    }
    Ok(())
}

/// Parse WHEEL file
///
/// > {distribution}-{version}.dist-info/WHEEL is metadata about the archive itself in the same
/// > basic key: value format:
fn parse_wheel_version(wheel_text: &str) -> Result<(), WheelInstallerError> {
    // {distribution}-{version}.dist-info/WHEEL is metadata about the archive itself in the same basic key: value format:
    // The proper solution would probably also be using the email parser here
    let wheel_file_data = wheel_text
        .lines()
        // Filter empty lines
        .filter(|line| !line.trim().is_empty())
        .map(|entry| {
            entry
                .split_once(": ")
                .map(|(key, value)| (key.to_string(), value.to_string()))
        })
        .collect::<Option<HashMap<String, String>>>()
        .ok_or_else(|| {
            WheelInstallerError::InvalidWheel(
                "The contents of the WHEEL file are invalid".to_string(),
            )
        })?;
    let wheel_version = wheel_file_data.get("Wheel-Version").ok_or_else(|| {
        WheelInstallerError::InvalidWheel("Wheel-Version missing in WHEEL file".to_string())
    })?;
    let wheel_version = wheel_version.split_once(".").ok_or_else(|| {
        WheelInstallerError::InvalidWheel("Invalid Wheel-Version in WHEEL file".to_string())
    })?;
    // pip has some test wheels that use that ancient version,
    // and technically we only need to check that the version is not higher
    if wheel_version == ("0", "1") {
        warn!("Ancient wheel version 0.1 (expected is 1.0)");
        return Ok(());
    }
    // Check that installer is compatible with Wheel-Version. Warn if minor version is greater, abort if major version is greater.
    // Wheel-Version: 1.0
    if wheel_version.0 != "1" {
        return Err(WheelInstallerError::InvalidWheel(format!(
            "Unsupported wheel major version (expected {}, got {})",
            1, wheel_version.0
        )));
    }
    if wheel_version.1 > "0" {
        eprint!(
            "Warning: Unsupported wheel minor version (expected {}, got {})",
            0, wheel_version.1
        );
    }
    Ok(())
}

/// Call `python -m compileall` to generate pyc file for the installed code
///
/// 2.f Compile any installed .py to .pyc. (Uninstallers should be smart enough to remove .pyc
/// even if it is not mentioned in RECORD.)
fn bytecode_compile(
    site_packages: &Path,
    unpacked_paths: Vec<PathBuf>,
    python_version: (u8, u8),
    record: &mut Vec<RecordEntry>,
) -> Result<(), WheelInstallerError> {
    // https://github.com/pypa/pip/blob/b5457dfee47dd9e9f6ec45159d9d410ba44e5ea1/src/pip/_internal/operations/install/wheel.py#L592-L603
    let py_source_paths: Vec<_> = unpacked_paths
        .into_iter()
        .filter(|path| {
            site_packages.join(path).is_file() && path.extension() == Some(&OsString::from("py"))
        })
        .collect();

    // > Read the file list and add each line that it contains to the list of files and directories
    // > to compile. If list is -, read lines from stdin.
    let mut bytecode_compiler = Command::new("python")
        .args(&["-m", "compileall", "-i", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(WheelInstallerError::PythonSubcommandError)?;

    // https://stackoverflow.com/questions/49218599/write-to-child-process-stdin-in-rust/49597789#comment120223107_49597789
    let mut child_stdin = bytecode_compiler
        .stdin
        .take()
        .expect("Child must have stdin");

    // Pass paths newline terminated to compileall
    for path in &py_source_paths {
        trace!("bytecode compiling {}", path.display());
        // There is no OsStr -> Bytes conversion on windows :o
        // https://stackoverflow.com/questions/43083544/how-can-i-convert-osstr-to-u8-vecu8-on-windows
        writeln!(&mut child_stdin, "{}", site_packages.join(path).display())
            .map_err(WheelInstallerError::PythonSubcommandError)?;
    }
    // Close stdin to finish and avoid indefinite blocking
    drop(child_stdin);

    let output = bytecode_compiler
        .wait_with_output()
        .map_err(WheelInstallerError::PythonSubcommandError)?;
    if !output.status.success() {
        // lossy because we want the error reporting to survive c̴̞̏ü̸̜̹̈́ŕ̴͉̈ś̷̤ė̵̤͋d̷͙̄ filenames in the zip
        return Err(WheelInstallerError::PythonSubcommandError(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Failed to run `python -m compileall`: {}\n---stdout:\n{}---stderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )));
    }

    // Add to RECORD
    for py_path in &py_source_paths {
        let pyc_path = py_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join("__pycache__")
            // Unwrap is save because we checked for an extension before
            .join(py_path.file_name().unwrap())
            .with_extension(format!(
                "cpython-{}{}.pyc",
                python_version.0, python_version.1
            ));
        if !site_packages.join(&pyc_path).is_file() {
            return Err(WheelInstallerError::PythonSubcommandError(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "Didn't find pyc generated by `python -m compileall`: {}",
                    pyc_path.display()
                ),
            )));
        }
        // 2.d Update distribution-1.0.dist-info/RECORD with the installed paths.

        // https://www.python.org/dev/peps/pep-0376/#record
        // > [..] a hash of the file's contents. Notice that pyc and pyo generated files don't have
        // > any hash because they are automatically produced from py files. So checking the hash of
        // > the corresponding py file is enough to decide if the file and its associated pyc or pyo
        // > files have changed.
        record.push(RecordEntry {
            path: pyc_path.display().to_string(),
            hash: None,
            size: None,
        })
    }

    Ok(())
}

/// Moves the files and folders in src to dest, updating the RECORD in the process
fn move_folder_recorded(
    src_dir: &Path,
    dest_dir: &Path,
    site_packages: &Path,
    record: &mut Vec<RecordEntry>,
) -> Result<(), WheelInstallerError> {
    if !dest_dir.is_dir() {
        fs::create_dir_all(&dest_dir)?;
    }
    for entry in WalkDir::new(&src_dir) {
        let entry = entry.map_err(WheelInstallerError::WalkDirError)?;
        let src = entry.path();
        // This is the base path for moving to the actual target for the data
        // e.g. for data it's without .data/data/
        let relative_to_data = src.strip_prefix(&src_dir).expect("Prefix must no change");
        // This is the path stored in RECORD
        // e.g. for data it's with .data/data/
        let relative_to_site_packages = src
            .strip_prefix(site_packages)
            .expect("Prefix must no change");
        let target = dest_dir.join(relative_to_data);
        if src.is_dir() {
            if !target.is_dir() {
                fs::create_dir(target)?;
            }
        } else {
            fs::rename(src, &target)?;
            let mut entry = record
                .iter_mut()
                .find(|entry| Path::new(&entry.path) == relative_to_site_packages)
                .ok_or_else(|| {
                    WheelInstallerError::RecordFileError(format!(
                        "Could not find entry for {} ({})",
                        relative_to_site_packages.display(),
                        src.display()
                    ))
                })?;
            entry.path = target.display().to_string();
        }
    }
    Ok(())
}

/// Installs a single script (not an entrypoint)
///
/// Has to deal with both binaries files (just move) and scripts (rewrite the shebang if applicable)
fn install_script(
    venv_base: &Path,
    site_packages: &Path,
    record: &mut Vec<RecordEntry>,
    file: DirEntry,
) -> Result<(), WheelInstallerError> {
    let path = file.path();
    if !path.is_file() {
        return Err(WheelInstallerError::InvalidWheel(format!(
            "Wheel contains entry in scripts directory that is not a file: {}",
            path.display()
        )));
    }

    let target_path = Path::new("../../../bin").join(file.file_name());
    let mut script = File::open(&path)?;
    // > In wheel, scripts are packaged in {distribution}-{version}.data/scripts/.
    // > If the first line of a file in scripts/ starts with exactly b'#!python',
    // > rewrite to point to the correct interpreter. Unix installers may need to
    // > add the +x bit to these files if the archive was created on Windows.
    //
    // > The b'#!pythonw' convention is allowed. b'#!pythonw' indicates a GUI script
    // > instead of a console script.
    let placeholder_python = b"#!python";
    // scripts might be binaries, so
    let mut start = Vec::new();
    start.resize(placeholder_python.len(), 0);
    script.read_exact(&mut start)?;
    let size_and_encoded_hash = if start == placeholder_python {
        start = format!("#!{}/bin/python", venv_base.canonicalize()?.display())
            .as_bytes()
            .to_vec();

        let mut target = File::create(site_packages.join(&target_path))?;
        let size_and_encoded_hash = copy_and_hash(&mut start.chain(script), &mut target)?;
        fs::remove_file(&path)?;
        Some(size_and_encoded_hash)
    } else {
        // reading and writing is slow especially for large binaries, so we move them instead
        drop(script);
        fs::rename(&path, &site_packages.join(&target_path))?;
        None
    };
    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(
            site_packages.join(&target_path),
            Permissions::from_mode(0o755),
        )?;
    }

    let relative_to_site_packages = path
        .strip_prefix(site_packages)
        .expect("Prefix must no change");
    let mut entry = record
        .iter_mut()
        .find(|entry| Path::new(&entry.path) == relative_to_site_packages)
        .ok_or_else(|| {
            // This should be possible to occur at this point, but filesystems and such
            WheelInstallerError::RecordFileError(format!(
                "Could not find entry for {} ({})",
                relative_to_site_packages.display(),
                path.display()
            ))
        })?;
    entry.path = target_path.display().to_string();
    if let Some((size, encoded_hash)) = size_and_encoded_hash {
        entry.size = Some(size as usize);
        entry.hash = Some(encoded_hash);
    }
    Ok(())
}

/// Move the files from the .data directory to the right location in the venv
fn install_data(
    venv_base: &Path,
    site_packages: &Path,
    data_dir: &Path,
    dist_name: &str,
    python_version: (u8, u8),
    record: &mut Vec<RecordEntry>,
) -> Result<(), WheelInstallerError> {
    for data_entry in fs::read_dir(data_dir)? {
        let data_entry = data_entry?;
        match data_entry.file_name().as_os_str().to_str() {
            Some("data") => {
                // Move the content of the folder to the root of the venv
                move_folder_recorded(&data_entry.path(), venv_base, site_packages, record)?;
            }
            Some("scripts") => {
                for file in fs::read_dir(data_entry.path())? {
                    let file = file?;
                    install_script(venv_base, site_packages, record, file)?;
                }
            }
            Some("headers") => {
                let target_path = venv_base.join(format!(
                    "include/site/python{}.{}/{}",
                    python_version.0, python_version.1, dist_name
                ));
                move_folder_recorded(&data_entry.path(), &target_path, site_packages, record)?;
            }
            Some("purelib" | "platlib") => {
                // TODO
                return Err(WheelInstallerError::InvalidWheel(
                    "purelib/platlib wheel data that is not supported yet".to_string(),
                ));
            }
            _ => {
                return Err(WheelInstallerError::InvalidWheel(format!(
                    "Unknown wheel data type: {:?}",
                    data_entry.file_name()
                )));
            }
        }
    }
    Ok(())
}

/// Write the content to a file and add the hash to the RECORD list
///
/// We still the path in the absolute path to the site packages and the relative path in the
/// site packages because we must only record the relative path in RECORD
fn write_file_recorded(
    site_packages: &Path,
    relative_path: &Path,
    content: &str,
    record: &mut Vec<RecordEntry>,
) -> Result<(), WheelInstallerError> {
    File::create(site_packages.join(relative_path))?.write_all(content.as_bytes())?;
    let hash = Sha256::new().chain_update(content.as_bytes()).finalize();
    let encoded_hash = format!(
        "sha256={}",
        base64::encode_config(&hash, base64::URL_SAFE_NO_PAD)
    );
    record.push(RecordEntry {
        path: relative_path.display().to_string(),
        hash: Some(encoded_hash),
        size: Some(content.as_bytes().len()),
    });
    Ok(())
}

/// Adds INSTALLER, REQUESTED and direct_url.json to the .dist-info dir
fn extra_dist_info(
    site_packages: &Path,
    dist_info: &Path,
    wheel_path: &Path,
    requested: bool,
    record: &mut Vec<RecordEntry>,
) -> Result<(), WheelInstallerError> {
    write_file_recorded(
        site_packages,
        &dist_info.join("INSTALLER"),
        env!("CARGO_PKG_NAME"),
        record,
    )?;
    if requested {
        write_file_recorded(site_packages, &dist_info.join("REQUESTED"), "", record)?;
    }

    let wheel_path_url = format!("file://{}", wheel_path.canonicalize()?.display());
    let direct_url = DirectUrl {
        archive_info: HashMap::new(),
        url: wheel_path_url,
    };
    // Map explicitly because we special cased that error
    let direct_url_json =
        serde_json::to_string(&direct_url).map_err(WheelInstallerError::DirectUrlSerdeJsonError)?;
    write_file_recorded(
        site_packages,
        &dist_info.join("direct_url.json"),
        &direct_url_json,
        record,
    )?;
    Ok(())
}

/// Reads the record file
/// https://www.python.org/dev/peps/pep-0376/#record
fn read_record_file(record: &mut impl Read) -> Result<Vec<RecordEntry>, WheelInstallerError> {
    csv::ReaderBuilder::new()
        .has_headers(false)
        .escape(Some(b'"'))
        .from_reader(record)
        .deserialize()
        .map(|x| {
            let y: RecordEntry = x?;
            Ok(y)
        })
        .collect()
}

/// Parse pyvenv.cfg from the root of the virtual env and returns the python major and minor version
fn get_python_version(pyvenv_cfg: &str) -> Result<(u8, u8), WheelInstallerError> {
    let pyvenv_cfg: HashMap<String, String> = pyvenv_cfg
        .lines()
        // Actual pyvenv.cfg doesn't have trailing newlines, but some program might insert some
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split_once(" = ")
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .ok_or_else(|| WheelInstallerError::BrokenVenv("Invalid pyvenv.cfg".to_string()))
        })
        .collect::<Result<HashMap<String, String>, WheelInstallerError>>()?;

    let version_info = pyvenv_cfg.get("version_info").ok_or_else(|| {
        WheelInstallerError::BrokenVenv("Missing version_info in pyvenv.cfg".to_string())
    })?;
    let python_version: (u8, u8) = match &version_info.split('.').collect::<Vec<_>>()[..] {
        [major, minor, ..] => (
            major.parse().map_err(|err| {
                WheelInstallerError::BrokenVenv(format!(
                    "Invalid major version_info in pyvenv.cfg: {}",
                    err
                ))
            })?,
            minor.parse().map_err(|err| {
                WheelInstallerError::BrokenVenv(format!(
                    "Invalid minor version_info in pyvenv.cfg: {}",
                    err
                ))
            })?,
        ),
        _ => {
            return Err(WheelInstallerError::BrokenVenv(
                "Invalid version_info in pyvenv.cfg".to_string(),
            ))
        }
    };
    Ok(python_version)
}

/// Install the given wheel to the given venv
///
/// https://packaging.python.org/en/latest/specifications/binary-distribution-format/#installing-a-wheel-distribution-1-0-py32-none-any-whl
///
/// Wheel 1.0: https://www.python.org/dev/peps/pep-0427/
pub(crate) fn install_wheel(
    venv_base: &Path,
    wheel_path: &Path,
    compile: bool,
) -> Result<(String, String), WheelInstallerError> {
    let filename = wheel_path
        .file_name()
        .ok_or_else(|| WheelInstallerError::InvalidWheel("Expected a file".to_string()))?
        .to_string_lossy();
    let name = filename
        .split_once("-")
        .ok_or_else(|| {
            WheelInstallerError::InvalidWheel(format!("Not a valid wheel filename: {}", filename))
        })?
        .0
        .to_owned();
    let _my_span = span!(Level::DEBUG, "install_wheel", name = name.as_str());

    let pyvenv_cfg = venv_base.join("pyvenv.cfg");
    if !pyvenv_cfg.is_file() {
        return Err(WheelInstallerError::BrokenVenv(format!(
            "The virtual environment needs to have a pyvenv.cfg, but {} doesn't exist",
            pyvenv_cfg.display(),
        )));
    }
    let python_version = get_python_version(&fs::read_to_string(pyvenv_cfg)?)?;

    let site_packages = venv_base
        .join("lib")
        .join(format!("python{}.{}", python_version.0, python_version.1))
        .join("site-packages");

    debug!(name = name.as_str(), "Getting wheel metadata");
    let dist = python_pkginfo::Distribution::new(&wheel_path)?;
    let escaped_name = Regex::new(r"[^\w\d.]+")
        .unwrap()
        .replace_all(&dist.metadata().name, "_");
    if escaped_name != name {
        return Err(WheelInstallerError::InvalidWheel(format!(
            "Inconsistent package name: {} vs {}",
            dist.metadata().name,
            name
        )));
    }
    let version = &dist.metadata().version;
    let dist_info_dir = format!("{}-{}.dist-info", escaped_name, version);

    debug!(name = name.as_str(), "Opening zip");
    let mut archive = ZipArchive::new(File::open(&wheel_path)?)?;

    debug!(name = name.as_str(), "Reading RECORD and WHEEL");
    let record_path = format!("{}/RECORD", dist_info_dir);
    let mut record = read_record_file(&mut archive.by_name(&record_path)?)?;

    // We're going step by step though
    // https://packaging.python.org/en/latest/specifications/binary-distribution-format/#installing-a-wheel-distribution-1-0-py32-none-any-whl
    // > 1.a Parse distribution-1.0.dist-info/WHEEL.
    // > 1.b Check that installer is compatible with Wheel-Version. Warn if minor version is greater, abort if major version is greater.
    let wheel_file_path = format!("{}/WHEEL", dist_info_dir);
    let mut wheel_text = String::new();
    archive
        .by_name(&wheel_file_path)?
        .read_to_string(&mut wheel_text)?;
    parse_wheel_version(&wheel_text)?;
    // > 1.c If Root-Is-Purelib == ‘true’, unpack archive into purelib (site-packages).
    // > 1.d Else unpack archive into platlib (site-packages).
    // We always install in the same virtualenv site packages
    debug!(name = name.as_str(), "Extracting file");
    let unpacked_paths = unpack_wheel_files(&site_packages, &record_path, &mut archive, &record)?;
    debug!(
        name = name.as_str(),
        "Extracted {} files",
        unpacked_paths.len()
    );

    let data_dir = site_packages.join(format!("{}-{}.data", escaped_name, version));
    // 2.a Unpacked archive includes distribution-1.0.dist-info/ and (if there is data) distribution-1.0.data/.
    // 2.b Move each subtree of distribution-1.0.data/ onto its destination path. Each subdirectory of distribution-1.0.data/ is a key into a dict of destination directories, such as distribution-1.0.data/(purelib|platlib|headers|scripts|data). The initially supported paths are taken from distutils.command.install.
    if data_dir.is_dir() {
        debug!(name = name.as_str(), "Installing data");
        install_data(
            venv_base,
            &site_packages,
            &data_dir,
            &name,
            python_version,
            &mut record,
        )?;
        // 2.c If applicable, update scripts starting with #!python to point to the correct interpreter.
        // Script are unsupported through data
        // 2.e Remove empty distribution-1.0.data directory.
        fs::remove_dir_all(data_dir)?;
    } else {
        debug!(name = name.as_str(), "No data");
    }

    // 2.f Compile any installed .py to .pyc. (Uninstallers should be smart enough to remove .pyc even if it is not mentioned in RECORD.)
    if compile {
        debug!(name = name.as_str(), "Bytecode compiling");
        bytecode_compile(&site_packages, unpacked_paths, python_version, &mut record)?;
    }

    debug!(
        name = name.as_str(),
        "Writing entrypoint and extra metadata"
    );
    let entrypoints = parse_console_scripts(&mut archive, &dist_info_dir)?;
    write_entrypoints(&site_packages, venv_base, entrypoints, &mut record)?;

    extra_dist_info(
        &site_packages,
        Path::new(&dist_info_dir),
        wheel_path,
        true,
        &mut record,
    )?;

    debug!(name = name.as_str(), "Writing record");
    let mut record_writer = csv::WriterBuilder::new()
        .has_headers(false)
        .escape(b'"')
        .from_path(site_packages.join(record_path))?;
    record.sort();
    for entry in record {
        record_writer.serialize(entry)?;
    }
    Ok((name.to_string(), version.to_string()))
}

#[cfg(test)]
mod test {
    use super::{get_python_version, parse_wheel_version};
    use indoc::{formatdoc, indoc};

    #[test]

    fn test_parse_wheel_version() {
        fn wheel_with_version(version: &str) -> String {
            return formatdoc! {"
                Wheel-Version: {}
                Generator: bdist_wheel (0.37.0)
                Root-Is-Purelib: true
                Tag: py2-none-any
                Tag: py3-none-any
                ",
                version
            };
        }
        parse_wheel_version(&wheel_with_version("1.0")).unwrap();
        parse_wheel_version(&wheel_with_version("2.0")).unwrap_err();
    }

    #[test]

    fn test_parse_pyenv_cfg() {
        let pyvenv_cfg = indoc! {"
            home = /usr
            implementation = CPython
            version_info = 3.8.10.final.0
            virtualenv = 20.11.2
            include-system-site-packages = false
            base-prefix = /usr
            base-exec-prefix = /usr
            base-executable = /usr/bin/python3
            ",
        };
        assert_eq!(get_python_version(pyvenv_cfg).unwrap(), (3, 8));
    }
}
