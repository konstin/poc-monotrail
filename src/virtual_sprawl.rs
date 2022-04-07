use crate::markers::Pep508Environment;
use crate::{compatible_tags, find_specs_to_install, install_specs, Arch, InstallLocation, Os};
use anyhow::Context;
use std::env::current_dir;
use std::path::{Path, PathBuf};

fn find_lockfile(file_running: &Path) -> Option<PathBuf> {
    let mut parent = if file_running.is_absolute() {
        file_running.parent().map(|path| path.to_path_buf())
    } else {
        file_running.parent().map(|relative| {
            current_dir()
                .unwrap_or_else(|_| PathBuf::from(""))
                .join(relative)
        })
    };

    while let Some(dir) = parent {
        if dir.join("pyproject.toml").exists() {
            return Some(dir.join("pyproject.toml"));
        }
        parent = dir.parent().map(|path| path.to_path_buf());
    }
    None
}

#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn setup_virtual_sprawl(
    file_running: &Path,
    python: &Path,
    python_version: (u8, u8),
    pep508_env: Option<Pep508Environment>,
) -> anyhow::Result<(String, Vec<(String, String)>)> {
    let virtual_sprawl_root = "/home/konsti/virtual_sprawl/virtual_sprawl".to_string();
    let pyproject_toml = find_lockfile(file_running).with_context(|| {
        format!(
            "pyproject.toml not found next to {} nor in any parent directory",
            file_running.display()
        )
    })?;
    let compatible_tags = compatible_tags(python_version, &Os::current()?, &Arch::current()?)?;
    let specs = find_specs_to_install(&pyproject_toml, false, &[], pep508_env)?;

    // ugly way to remove already installed
    let mut to_install_specs = Vec::new();
    let mut installed_done = Vec::new();
    for spec in specs {
        let version = spec
            .version
            .clone()
            .context("Missing version field in locked specs")?;
        let location = format!("{}-{}", spec.name.to_lowercase().replace('-', "_"), version);
        if Path::new(&virtual_sprawl_root).join(location).is_dir() {
            installed_done.push((spec.name, version));
        } else {
            to_install_specs.push(spec)
        }
    }

    let mut installed = install_specs(
        &to_install_specs,
        &InstallLocation::VirtualSprawl {
            virtual_sprawl_root: PathBuf::from(&virtual_sprawl_root),
            python: python.to_path_buf(),
            python_version,
        },
        &compatible_tags,
        false,
    )?;

    installed.extend(installed_done);

    let packages = installed
        .into_iter()
        .map(|(name, version)| (name.to_lowercase().replace('-', "_"), version))
        .collect();

    Ok((virtual_sprawl_root, packages))
}
