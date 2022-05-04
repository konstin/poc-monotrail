//! Parsing of pyproject.toml and poetry.lock

use crate::markers::Pep508Environment;
use crate::poetry::poetry_lock::PoetryLock;
use crate::poetry::poetry_toml::PoetryPyprojectToml;
use crate::poetry::{poetry_lock, poetry_toml};
use crate::spec::{DistributionType, RequestedSpec, SpecSource};
use anyhow::{bail, Context};
use fs_err as fs;
use install_wheel_rs::{WheelFilename, WheelInstallerError};
use regex::Regex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::str::FromStr;

/// Resolves a single package's filename and url inside a poetry lockfile
///
/// doesn't work because the pypi api wants a different python version than the one in the wheel
/// filename
#[allow(dead_code)]
pub fn filename_and_url(
    lockfile: &PoetryLock,
    package: &poetry_lock::Package,
    compatible_tags: &[(String, String, String)],
) -> anyhow::Result<(String, DistributionType, String)> {
    let hashed_files = lockfile
        .metadata
        .files
        .get(&package.name)
        .context("invalid lockfile (missing file hashes), run `poetry update`")?;
    let filenames: Vec<_> = hashed_files
        .iter()
        .filter(|hashed_file| hashed_file.file.ends_with(".whl"))
        .map(|hashed_file| {
            Ok((
                hashed_file.file.clone(),
                WheelFilename::from_str(&hashed_file.file).with_context(|| {
                    format!(
                        "Couldn't parse wheel filename {} in lockfile",
                        hashed_file.file
                    )
                })?,
            ))
        })
        .collect::<Result<_, anyhow::Error>>()?;
    let wheel = filenames
        .iter()
        .find(|(_filename, parsed)| parsed.is_compatible(compatible_tags));

    if let Some((filename, parsed_filename)) = wheel {
        // https://warehouse.pypa.io/api-reference/integration-guide.html#if-you-so-choose
        let url = format!(
            "https://files.pythonhosted.org/packages/{}/{}/{}/{}",
            parsed_filename.python_tag.join("."),
            package.name.chars().next().unwrap(),
            package.name,
            filename,
        );
        return Ok((filename.clone(), DistributionType::Wheel, url));
    }

    if let Some(hashed_file) = hashed_files
        .iter()
        .find(|hashed_file| hashed_file.file.ends_with(".tar.gz"))
    {
        // https://warehouse.pypa.io/api-reference/integration-guide.html#if-you-so-choose
        let url = format!(
            "https://files.pythonhosted.org/packages/{}/{}/{}/{}",
            "source",
            package.name.chars().next().unwrap(),
            package.name,
            hashed_file.file,
        );
        Ok((
            hashed_file.file.clone(),
            DistributionType::SourceDistribution,
            url,
        ))
    } else {
        bail!(
            "No compatible compiled file found for {}. \
                Why does it have neither a wheel for your operating system/architecture/python version not any sdist?",
            package.name
        )
    }
}

/// this isn't actually needed poetry gives us all we need
#[allow(dead_code)]
fn parse_dep_extra(
    dep_spec: &str,
) -> Result<(String, HashSet<String>, Option<String>), WheelInstallerError> {
    let re = Regex::new(r"(?P<name>[\w\d_-]+)(?:\[(?P<extras>.*)\])? ?(?:\((?P<version>.+)\))?")
        .unwrap();
    let captures = re.captures(dep_spec).ok_or_else(|| {
        WheelInstallerError::InvalidWheel(format!(
            "Invalid dependency specification in poetry.lock: {}",
            dep_spec
        ))
    })?;
    Ok((
        captures.name("name").unwrap().as_str().to_string(),
        captures
            .name("extras")
            .map(|extras| {
                extras
                    .as_str()
                    .to_string()
                    .split(',')
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        captures
            .name("version")
            .map(|version| version.as_str().to_string()),
    ))
}

fn resolution_to_specs(
    packages: HashMap<String, poetry_lock::Package>,
    deps_with_extras: HashMap<String, HashSet<String>>,
) -> anyhow::Result<Vec<RequestedSpec>> {
    let mut specs = Vec::new();
    for (dep_name, dep_extras) in deps_with_extras {
        // search by normalized name
        let package = packages
            .get(&dep_name.to_lowercase().replace('-', "_"))
            .with_context(|| {
                format!(
                    "Lockfile outdated (run `poetry update`): {} is missing",
                    dep_name
                )
            })?;
        let spec = RequestedSpec {
            requested: format!("{} {}", package.name, package.version),
            name: package.name.clone(),
            python_version: Some(package.version.clone()),
            source: package.source.clone().map(|source| SpecSource {
                source_type: source.source_type,
                url: source.url,
                reference: source.reference,
                resolved_reference: source.resolved_reference,
            }),
            extras: dep_extras.into_iter().collect(),
            file_path: None,
            url: None,
        };
        specs.push(spec);
    }
    Ok(specs)
}

/// Get the root deps from pyproject.toml, already filtered by activated extras.
/// The is no root package in poetry.lock that we could use so we also need to read pyproject.toml
fn get_root_info(
    pyproject_toml: &PoetryPyprojectToml,
    no_dev: bool,
    extras: &[String],
) -> anyhow::Result<HashMap<String, poetry_toml::Dependency>> {
    let poetry_section = pyproject_toml.tool.poetry.clone();

    let root_deps = if no_dev {
        poetry_section.dependencies.clone()
    } else {
        poetry_section
            .dependencies
            .into_iter()
            .chain(poetry_section.dev_dependencies)
            .collect()
    };

    let mut root_extra_deps: HashSet<_> = HashSet::new();
    for extra_name in extras {
        let packages = poetry_section
            .extras
            .get(extra_name)
            .with_context(|| format!("No such extra {}", extra_name))?;
        root_extra_deps.extend(packages);
    }

    let root_deps = root_deps
        .into_iter()
        .filter(|(dep_name, dep_spec)| {
            // We do not need to install python (oh if we only could, relocatable python a dream)
            if dep_name == "python" {
                return false;
            }
            // Use only those optional deps which are activated by a selected extra
            if dep_spec.is_optional() && !root_extra_deps.contains(&dep_name) {
                return false;
            }
            true
        })
        .collect();

    Ok(root_deps)
}

fn get_packages_from_lockfile(
    poetry_lock: &PoetryLock,
) -> anyhow::Result<HashMap<String, poetry_lock::Package>> {
    // keys are normalized names since `[package.dependencies]` also uses normalized names
    let packages: HashMap<String, poetry_lock::Package> = poetry_lock
        .package
        .clone()
        .into_iter()
        .map(|package| (package.name.replace('-', "_"), package))
        .collect();
    Ok(packages)
}

/// Reads pyproject.toml and poetry.lock
pub fn read_toml_files(dir: &Path) -> anyhow::Result<(PoetryPyprojectToml, PoetryLock)> {
    let poetry_toml = toml::from_str(&fs::read_to_string(dir.join("pyproject.toml"))?)
        .context("Invalid pyproject.toml")?;
    let poetry_lock = toml::from_str(&fs::read_to_string(dir.join("poetry.lock"))?)
        .context("Invalid pyproject.toml")?;
    Ok((poetry_toml, poetry_lock))
}

/// Parses pyproject.toml and poetry.lock and returns a list of packages to install
pub fn read_poetry_specs(
    pyproject_toml: PoetryPyprojectToml,
    poetry_lock: PoetryLock,
    no_dev: bool,
    extras: &[String],
    pep508_env: &Pep508Environment,
) -> anyhow::Result<Vec<RequestedSpec>> {
    // The deps in pyproject.toml which we need to read explicitly since they aren't marked
    // poetry.lock (raw names)
    let root_deps = get_root_info(&pyproject_toml, no_dev, extras)?;
    // All the details info from poetry.lock, indexed by normalized name
    let packages = get_packages_from_lockfile(&poetry_lock)?;

    // This is the thing we want to build: a list with all transitive dependencies and
    // all their (transitively activated) features
    let mut deps_with_extras: HashMap<String, HashSet<String>> = HashMap::new();
    // (dep, dep->extra) combinations we still need to process
    let mut queue: VecDeque<(String, HashSet<String>)> = VecDeque::new();
    // Since we have no explicit root package, prime manually
    for (dep_name, dep_spec) in root_deps {
        let dep_name_norm = dep_name.to_lowercase().replace('-', "_");

        queue.push_back((
            dep_name_norm.clone(),
            dep_spec.get_extras().iter().cloned().collect(),
        ));
        deps_with_extras.insert(
            dep_name_norm.clone(),
            dep_spec.get_extras().iter().cloned().collect(),
        );
    }

    // resolve the dependencies-extras tree
    // (dep, dep->extra)
    while let Some((dep_name, self_extras)) = queue.pop_front() {
        let package = packages
            .get(&dep_name.to_lowercase().replace('-', "_"))
            .with_context(|| {
                format!(
                    "Lockfile outdated (run `poetry update`): {} is missing",
                    dep_name
                )
            })?;
        // descend one level into the dep tree
        for (new_dep_name, new_dep) in package.dependencies.clone().unwrap_or_default() {
            let new_dep_name_norm = new_dep_name.to_lowercase().replace('-', "_");
            // Check the extras selected on the current dep activate the transitive dependency
            let (_new_dep_version, new_dep_extras) = match new_dep
                .get_version_and_extras(pep508_env, &self_extras)
                .map_err(WheelInstallerError::InvalidPoetry)?
            {
                None => continue,
                Some((version, new_dep_extras)) => (version, new_dep_extras),
            };

            let new_dep_extras: HashSet<String> = new_dep_extras.into_iter().collect();

            let new_extras = if let Some(known_extras) = deps_with_extras.get(&new_dep_name) {
                if new_dep_extras.is_subset(known_extras) {
                    // nothing to do here, the dep and all those extras are already known
                    continue;
                } else {
                    new_dep_extras.difference(known_extras).cloned().collect()
                }
            } else {
                new_dep_extras
            };

            deps_with_extras
                .entry(new_dep_name_norm.clone())
                .or_default()
                .extend(new_extras.clone());
            queue.push_back((new_dep_name_norm.clone(), new_extras));
        }
    }

    let specs = resolution_to_specs(packages, deps_with_extras)?;
    Ok(specs)
}

#[cfg(test)]
mod test {
    use crate::markers::Pep508Environment;
    use crate::poetry::read_dependencies::{parse_dep_extra, read_toml_files};
    use crate::read_poetry_specs;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn test_parse_extra_deps() {
        let examples = [
            ("pympler", ("pympler".to_string(), HashSet::new(), None)),
            (
                "pytest (>=4.3.0)",
                (
                    "pytest".to_string(),
                    HashSet::new(),
                    Some(">=4.3.0".to_string()),
                ),
            ),
            (
                "coverage[toml] (>=5.0.2)",
                (
                    "coverage".to_string(),
                    ["toml".to_string()].into_iter().collect(),
                    Some(">=5.0.2".to_string()),
                ),
            ),
        ];

        for (input, expected) in examples {
            assert_eq!(parse_dep_extra(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_read_poetry_specs() {
        let pep508_env = Pep508Environment {
            implementation_name: "cpython".to_string(),
            implementation_version: "3.8.10".to_string(),
            os_name: "posix".to_string(),
            platform_machine: "x86_64".to_string(),
            platform_python_implementation: "CPython".to_string(),
            platform_release: "5.13.0-39-generic".to_string(),
            platform_system: "Linux".to_string(),
            platform_version: "#44~20.04.1-Ubuntu SMP Thu Mar 24 16:43:35 UTC 2022".to_string(),
            python_full_version: "3.8.10".to_string(),
            python_version: "3.8".to_string(),
            sys_platform: "linux".to_string(),
        };
        let mst = Path::new("test-data/poetry/mst");
        let data_science = Path::new("test-data/poetry/data-science");

        let expected = [
            (mst, true, vec![], 95),
            (mst, false, vec![], 130),
            (mst, true, vec!["import-json".to_string()], 97),
            (mst, false, vec!["import-json".to_string()], 131),
            (data_science, true, vec![], 14),
            (data_science, false, vec![], 20),
            (data_science, true, vec!["tqdm_feature".to_string()], 15),
            (data_science, false, vec!["tqdm_feature".to_string()], 21),
        ];

        for (toml_dir, no_dev, extras, specs_count) in expected {
            let (poetry_toml, poetry_lock) = read_toml_files(toml_dir).unwrap();
            let specs =
                read_poetry_specs(poetry_toml, poetry_lock, no_dev, &extras, &pep508_env).unwrap();
            assert_eq!(specs.len(), specs_count);
        }
    }
}
