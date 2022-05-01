//! Multiplexing between venv install and monotrail install

use crate::install::InstalledPackage;
use crate::monotrail::filter_installed_monotrail;
use crate::spec::RequestedSpec;
use crate::wheel::parse_key_value_file;
use anyhow::Context;
use fs2::FileExt;
use fs_err as fs;
use fs_err::{DirEntry, File};
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use tracing::{error, warn};

const MONOTRAIL_LOCKFILE: &str = "monotrail.lock";

/// A directory for which we acquired a monotrail.lock lockfile
pub struct LockedDir {
    /// The directory to lock
    path: PathBuf,
    /// handle on the monotrail.lock that drops the lock
    lockfile: File,
}

impl LockedDir {
    /// Tries to lock the directory, returns Ok(None) if it is already locked
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        let lockfile = File::create(path.join(MONOTRAIL_LOCKFILE))?;
        if lockfile.file().try_lock_exclusive().is_ok() {
            Ok(Some(Self {
                path: path.to_path_buf(),
                lockfile,
            }))
        } else {
            Ok(None)
        }
    }

    /// Locks the directory, if necessary blocking until the lock becomes free
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let lockfile = File::create(path.join(MONOTRAIL_LOCKFILE))?;
        lockfile.file().lock_exclusive()?;
        Ok(Self {
            path: path.to_path_buf(),
            lockfile,
        })
    }
}

impl Drop for LockedDir {
    fn drop(&mut self) {
        if let Err(err) = self.lockfile.file().unlock() {
            error!(
                "Failed to unlock {}: {}",
                self.lockfile.path().display(),
                err
            );
        }
    }
}

impl Deref for LockedDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

/// Multiplexing between venv install and monotrail install
///
/// For monotrail, we have a structure that is {monotrail}/{normalized(name)}/{version}/tag
///
/// We use a lockfile to prevent multiple instance writing stuff on the same time
/// As of pip 22.0, e.g. `pip install numpy; pip install numpy; pip install numpy` will
/// nondeterministically fail
///
/// I was also thinking about making a shared lock on the import side, but monotrail install
/// is supposedly atomic (by directory renaming), while for venv installation there can't be
/// atomicity (we need to add lots of different file without a top level directory / key-turn
/// file we could rename) and the locking would also need to happen in the import mechanism
/// itself to ensure
pub enum InstallLocation<T: Deref<Target = Path>> {
    Venv {
        /// absolute path
        venv_base: T,
        python_version: (u8, u8),
    },
    Monotrail {
        monotrail_root: T,
        python: PathBuf,
        python_version: (u8, u8),
    },
}

impl<T: Deref<Target = Path>> InstallLocation<T> {
    /// Returns the location of the python interpreter
    pub fn get_python(&self) -> PathBuf {
        match self {
            InstallLocation::Venv { venv_base, .. } => {
                // canonicalize on python would resolve the symlink
                venv_base.join("bin").join("python")
            }
            // TODO: For monotrail use the monotrail launcher
            InstallLocation::Monotrail { python, .. } => python.clone(),
        }
    }

    pub fn get_python_version(&self) -> (u8, u8) {
        match self {
            InstallLocation::Venv { python_version, .. } => *python_version,
            InstallLocation::Monotrail { python_version, .. } => *python_version,
        }
    }

    pub fn filter_installed(
        &self,
        specs: &[RequestedSpec],
    ) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
        match self {
            InstallLocation::Venv {
                venv_base,
                python_version,
            } => filter_installed_venv(specs, venv_base, *python_version).context(format!(
                "Failed to filter packages installed in the venv at {}",
                venv_base.display()
            )),
            InstallLocation::Monotrail { monotrail_root, .. } => {
                Ok(filter_installed_monotrail(specs, monotrail_root)?)
            }
        }
    }

    pub fn is_installed(&self, normalized_name: &str, version: &str) -> bool {
        match self {
            InstallLocation::Venv {
                venv_base,
                python_version,
            } => venv_base
                .join("lib")
                .join(format!("python{}.{}", python_version.0, python_version.1))
                .join("site-packages")
                .join(format!("{}-{}.dist-info", normalized_name, version))
                .is_dir(),
            InstallLocation::Monotrail { monotrail_root, .. } => monotrail_root
                .join(format!("{}-{}", normalized_name, version))
                .is_dir(),
        }
    }
}

impl InstallLocation<PathBuf> {
    pub fn acquire_lock(&self) -> io::Result<InstallLocation<LockedDir>> {
        let root = match self {
            Self::Venv { venv_base, .. } => venv_base,
            Self::Monotrail { monotrail_root, .. } => monotrail_root,
        };

        // If necessary, create monotrail dir
        fs::create_dir_all(root)?;

        let locked_dir = if let Some(locked_dir) = LockedDir::try_acquire(root)? {
            locked_dir
        } else {
            warn!(
                "Could not acquire exclusive lock for installing, is another installation process \
                running? Sleeping until lock becomes free"
            );
            LockedDir::acquire(root)?
        };

        Ok(match self {
            Self::Venv { python_version, .. } => InstallLocation::Venv {
                venv_base: locked_dir,
                python_version: *python_version,
            },
            Self::Monotrail {
                python_version,
                python,
                ..
            } => InstallLocation::Monotrail {
                monotrail_root: locked_dir,
                python: python.clone(),
                python_version: *python_version,
            },
        })
    }
}

/// Reads the installed packages through .dist-info/WHEEL files, returns the set that is installed
/// and the one that still needs to be installed
pub fn filter_installed_venv(
    specs: &[RequestedSpec],
    venv_base: &Path,
    python_version: (u8, u8),
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    let entries: Vec<DirEntry> = match fs::read_dir(
        venv_base
            .join("lib")
            .join(format!("python{}.{}", python_version.0, python_version.1))
            .join("site-packages"),
    ) {
        Ok(entries) => entries.collect::<io::Result<Vec<DirEntry>>>()?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let venv_packages: Vec<InstalledPackage> = entries
        .iter()
        .filter_map(|entry| {
            let filename = entry.file_name().to_string_lossy().to_string();
            let (name, version) = filename.strip_suffix(".dist-info")?.split_once('-')?;
            let name = name.to_lowercase().replace('-', "_");
            Some((entry, name, version.to_string()))
        })
        .map(|(entry, name, version)| {
            let wheel_data =
                parse_key_value_file(&mut File::open(entry.path().join("WHEEL"))?, "WHEEL")?;
            let tag = wheel_data
                .get("Tag")
                .map(|tags| tags.join("."))
                .unwrap_or_default();

            Ok(InstalledPackage {
                name,
                python_version: version.clone(),
                unique_version: version,
                tag,
            })
        })
        .collect::<anyhow::Result<_>>()?;

    let mut installed = Vec::new();
    let mut not_installed = Vec::new();
    for spec in specs {
        let matching_package = venv_packages.iter().find(|package| {
            if let Some(spec_version) = &spec.python_version {
                // TODO: use PEP440
                package.name == spec.name && &package.python_version == spec_version
            } else {
                package.name == spec.name
            }
        });
        if let Some(package) = matching_package {
            installed.push(package.clone());
        } else {
            not_installed.push(spec.clone())
        }
    }
    Ok((not_installed, installed))
}
