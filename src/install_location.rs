use crate::wheel_tags::WheelFilename;
use std::io;
use std::path::PathBuf;

pub enum InstallLocation {
    Venv {
        venv_base: PathBuf,
        python_version: (u8, u8),
    },
    VirtualSprawl {
        virtual_sprawl_root: PathBuf,
        python: PathBuf,
        python_version: (u8, u8),
    },
}

impl InstallLocation {
    pub fn get_python(&self) -> io::Result<PathBuf> {
        Ok(match self {
            InstallLocation::Venv { venv_base, .. } => {
                // canonicalize on python would resolve the symlink
                venv_base.canonicalize()?.join("bin").join("python")
            }
            // TODO: For virtual_sprawl use the virtual_sprawl launcher
            InstallLocation::VirtualSprawl { python, .. } => python.clone(),
        })
    }

    pub fn get_python_version(&self) -> (u8, u8) {
        match self {
            InstallLocation::Venv { python_version, .. } => *python_version,
            InstallLocation::VirtualSprawl { python_version, .. } => *python_version,
        }
    }

    pub fn get_install_location(&self, filename: &WheelFilename) -> PathBuf {
        match self {
            InstallLocation::Venv { venv_base, .. } => venv_base.to_path_buf(),
            InstallLocation::VirtualSprawl {
                virtual_sprawl_root,
                ..
            } => {
                // TODO: The version needs to be passed otherwise so we can also handle git hashes
                // and such
                virtual_sprawl_root.join(format!(
                    "{}-{}",
                    filename.distribution.to_lowercase(),
                    filename.version
                ))
            }
        }
    }

    pub fn is_installed(&self, name: &str, version: &str) -> bool {
        match self {
            InstallLocation::Venv {
                venv_base,
                python_version,
            } => venv_base
                .join("lib")
                .join(format!("python{}.{}", python_version.0, python_version.1))
                .join("site-packages")
                .join(format!(
                    "{}-{}.dist-info",
                    name.to_lowercase().replace('-', "_"),
                    version
                ))
                .is_dir(),
            InstallLocation::VirtualSprawl {
                virtual_sprawl_root,
                ..
            } => virtual_sprawl_root
                .join(format!(
                    "{}-{}",
                    name.to_lowercase().replace('-', "_"),
                    version
                ))
                .is_dir(),
        }
    }
}
