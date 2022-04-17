//! Multiplexing between venv install and virtual sprawl install

use fs2::FileExt;
use fs_err::File;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::{fs, io};
use tracing::{error, warn};

const VIRTUAL_SPRAWL_LOCKFILE: &'static str = "virtual-sprawl.lock";

/// A directory for which we acquired a virtual-sprawl.lock lockfile
pub struct LockedDir {
    /// The directory to lock
    path: PathBuf,
    /// handle on the virtual-sprawl.lock that drops the lock
    lockfile: File,
}

impl LockedDir {
    /// Tries to lock the directory, returns Ok(None) if it is already locked
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        let lockfile = File::create(path.join(VIRTUAL_SPRAWL_LOCKFILE))?;
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
        let lockfile = File::create(path.join(VIRTUAL_SPRAWL_LOCKFILE))?;
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

/// Multiplexing between venv install and virtual sprawl install
///
/// For virtual sprawl, we have a structure that is {virtual sprawl}/{normalized(name)}/{version}/tag
///
/// We use a lockfile to prevent multiple instance writing stuff on the same time
/// As of pip 22.0, e.g. `pip install numpy; pip install numpy; pip install numpy` will
/// nondeterministically fail
///
/// I was also thinking about making a shared lock on the import side, but virtual sprawl install
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
    VirtualSprawl {
        virtual_sprawl_root: T,
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
            // TODO: For virtual_sprawl use the virtual_sprawl launcher
            InstallLocation::VirtualSprawl { python, .. } => python.clone(),
        }
    }

    pub fn get_python_version(&self) -> (u8, u8) {
        match self {
            InstallLocation::Venv { python_version, .. } => *python_version,
            InstallLocation::VirtualSprawl { python_version, .. } => *python_version,
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
            InstallLocation::VirtualSprawl {
                virtual_sprawl_root,
                ..
            } => virtual_sprawl_root
                .join(format!("{}-{}", normalized_name, version))
                .is_dir(),
        }
    }
}

impl InstallLocation<PathBuf> {
    pub fn acquire_lock(&self) -> io::Result<InstallLocation<LockedDir>> {
        let root = match self {
            Self::Venv { venv_base, .. } => venv_base,
            Self::VirtualSprawl {
                virtual_sprawl_root,
                ..
            } => virtual_sprawl_root,
        };

        // If necessary, create virtual sprawl dir
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
            Self::VirtualSprawl {
                python_version,
                python,
                ..
            } => InstallLocation::VirtualSprawl {
                virtual_sprawl_root: locked_dir,
                python: python.clone(),
                python_version: *python_version,
            },
        })
    }
}
