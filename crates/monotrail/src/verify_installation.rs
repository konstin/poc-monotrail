//! Subcommand to check the monotrail installations against their records

use crate::monotrail::list_installed;
use crate::utils::get_dir_content;
use anyhow::{bail, format_err, Context};
use data_encoding::BASE64URL_NOPAD;
use fs_err as fs;
use fs_err::File;
use indicatif::ProgressBar;
use install_wheel_rs::{read_record_file, relative_to};
use pep508_rs::PackageName;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use tracing::debug;
use walkdir::WalkDir;

/// Checks a single package in `root` against its RECORD
fn verify_package(
    root: &Path,
    name: &PackageName,
    unique_version: &str,
    tag: &str,
) -> anyhow::Result<Vec<String>> {
    let mut failing = Vec::new();
    let package_root = root.join(name.to_string()).join(unique_version).join(tag);
    let site_packages = if cfg!(windows) {
        package_root.join("Lib").join("site-packages")
    } else {
        package_root
            .join("lib")
            .join("python")
            .join("site-packages")
    };

    // detect the location of the <name>-<version>.dist-info directory
    let dist_info: Vec<_> = get_dir_content(&site_packages)
        .context("Failed to read site packages")?
        .iter()
        .filter(|dir| {
            // We don't know the python here, so we match <name>(.*)\.dist-info for
            // <name>-<version>.dist-info
            dir.file_name()
                .to_string_lossy()
                // normalize package name
                .to_lowercase()
                .replace('-', "_")
                .starts_with(&name.to_string())
                && dir.file_name().to_string_lossy().ends_with(".dist-info")
        })
        .map(|entry| entry.path())
        .collect();
    let dist_info = match dist_info.as_slice() {
        &[] => {
            bail!(
                "No .dist-info found for {} {} {} in {}",
                name,
                unique_version,
                tag,
                site_packages.display()
            );
        }
        [dist_info] => dist_info.clone(),
        more => {
            bail!(
                "Multiple .dist-info found for {} {} {} in {}: {:?}",
                name,
                unique_version,
                tag,
                site_packages.display(),
                more
            );
        }
    };
    let record =
        fs::read_to_string(dist_info.join("RECORD")).context("Couldn't read RECORD file")?;
    let record = read_record_file(&mut record.as_bytes()).context("Invalid RECORD file")?;

    // Collect files on disk and their hashes
    let mut on_disk = HashMap::new();
    for entry in WalkDir::new(&package_root) {
        let entry = entry.context("walkdir failed")?;
        // If it's neither dir nor file we want to fail
        if entry.file_type().is_dir() {
            continue;
        }
        let mut hasher = Sha256::new();
        let mut file = File::open(entry.path()).context("Failed to open file for hashing")?;
        io::copy(&mut file, &mut hasher).context("Failed to read file for hashing")?;
        let hash = format!("sha256={}", BASE64URL_NOPAD.encode(&hasher.finalize()));
        let record_path = relative_to(&entry.path(), &site_packages)?;
        let file_name = record_path
            .to_str()
            .with_context(|| format_err!("non-utf8 path: {:?}", record_path))?
            .to_string();
        on_disk.insert(file_name, hash);
    }

    let record_map: HashMap<_, _> = record
        .iter()
        .map(|record| (&record.path, &record.hash))
        .collect();
    debug!(
        "Package {} {} {} has {} disk entries and {} record entries",
        name,
        unique_version,
        tag,
        on_disk.len(),
        record_map.len()
    );

    // Here we check that all that in record are also on disk and that their hashes match
    for (entry_file, entry_hash) in &record_map {
        // Some (e.g. RECORD itself) don't have a hash entry
        let entry_hash = if let Some(entry_hash) = entry_hash {
            entry_hash
        } else {
            continue;
        };
        let disk_hash = if let Some(disk_hash) = on_disk.get(*entry_file) {
            disk_hash
        } else {
            debug!("Missing file {}", entry_file);
            failing.push(entry_file.to_string());
            continue;
        };

        if entry_hash != disk_hash {
            debug!(
                "Hash mismatch for {}: {} vs {}",
                entry_file, entry_hash, disk_hash
            );
        }
    }

    // Are there files on disk that are not in the record
    for extra_on_disk in on_disk
        .keys()
        .filter(|&file| !record_map.contains_key(file))
        // Since we run multiple python versions on the same code, python may add pyc files
        // from different versions that we didn't record when installing
        .filter(|&file| !file.ends_with(".pyc"))
    {
        debug!("Extra file on disk: {}", extra_on_disk);
        failing.push(extra_on_disk.to_string());
    }

    Ok(failing)
}

/// Checks all installed packages against their RECORD
pub fn verify_installation(root: &Path) -> anyhow::Result<Vec<String>> {
    let installed = list_installed(root, None).context("Failed to collect installed packages")?;
    let bar = ProgressBar::new(installed.len() as u64);
    let failing = installed
        // 5.7s iter/explicit loop vs 1.6s par_iter on my laptop
        .par_iter()
        .map(|(name, unique_version, tag)| {
            let failing = verify_package(root, &name, &unique_version, &tag)?;
            bar.inc(1);
            Ok(failing)
        })
        // TODO: Error handling that flattens while keeping the error
        .collect::<anyhow::Result<Vec<Vec<String>>>>()?
        .into_iter()
        .flatten()
        .collect();
    bar.finish();
    Ok(failing)
}
