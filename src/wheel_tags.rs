//! Parses the wheel filename, the current host os/arch and checks wheels for compatibility

use crate::WheelInstallerError;
use anyhow::Context;
use anyhow::{anyhow, Result};
use fs_err as fs;
use goblin::elf::Elf;
use platform_info::{PlatformInfo, Uname};
use regex::Regex;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

#[derive(Debug)]
pub struct WheelFilename {
    pub distribution: String,
    pub version: String,
    pub python_tag: Vec<String>,
    pub abi_tag: Vec<String>,
    pub platform_tag: Vec<String>,
}

impl FromStr for WheelFilename {
    type Err = WheelInstallerError;

    fn from_str(filename: &str) -> Result<Self, Self::Err> {
        let basename = filename.strip_suffix(".whl").ok_or_else(|| {
            WheelInstallerError::InvalidWheelFileName(
                filename.to_string(),
                "Must end with .whl".to_string(),
            )
        })?;
        // https://www.python.org/dev/peps/pep-0427/#file-name-convention
        match basename.split('-').collect::<Vec<_>>().as_slice() {
            // TODO: Build tag precedence
            &[distribution, version, _, python_tag, abi_tag, platform_tag]
            | &[distribution, version, python_tag, abi_tag, platform_tag] => Ok(WheelFilename {
                distribution: distribution.to_string(),
                version: version.to_string(),
                python_tag: python_tag.split('.').map(String::from).collect(),
                abi_tag: abi_tag.split('.').map(String::from).collect(),
                platform_tag: platform_tag.split('.').map(String::from).collect(),
            }),
            _ => Err(WheelInstallerError::InvalidWheelFileName(
                filename.to_string(),
                "Expected four \"-\" in the filename".to_string(),
            )),
        }
    }
}

impl WheelFilename {
    pub fn is_compatible(&self, compatible_tags: &[(String, String, String)]) -> bool {
        for tag in compatible_tags {
            if self.python_tag.contains(&tag.0)
                && self.abi_tag.contains(&tag.1)
                && self.platform_tag.contains(&tag.2)
            {
                return true;
            }
        }
        false
    }
}

/// Returns the compatible tags in a (python_tag, abi_tag, platform_tag) format
pub fn compatible_tags(
    python_version: (u8, u8),
    os: &Os,
    arch: &Arch,
) -> Result<Vec<(String, String, String)>, WheelInstallerError> {
    assert_eq!(python_version.0, 3);
    let mut tags = Vec::new();
    let platform_tags = compatible_platform_tags(os, arch)?;
    // 1. This exact c api version
    for platform_tag in &platform_tags {
        tags.push((
            format!("cp{}{}", python_version.0, python_version.1),
            format!("cp{}{}", python_version.0, python_version.1),
            platform_tag.clone(),
        ));
        tags.push((
            format!("cp{}{}", python_version.0, python_version.1),
            "none".to_string(),
            platform_tag.clone(),
        ));
    }
    // 2. abi3 and no abi (e.g. executable binary)
    // For some reason 3.2 is the minimum python for the cp abi
    for minor in 2..=python_version.1 {
        for platform_tag in &platform_tags {
            tags.push((
                format!("cp{}{}", python_version.0, minor),
                "abi3".to_string(),
                platform_tag.clone(),
            ));
        }
    }
    // 3. no abi (e.g. executable binary)
    for minor in 0..=python_version.1 {
        for platform_tag in &platform_tags {
            tags.push((
                format!("py{}{}", python_version.0, minor),
                "none".to_string(),
                platform_tag.clone(),
            ));
        }
    }
    // 4. major only
    for platform_tag in platform_tags {
        tags.push((
            format!("py{}", python_version.0),
            "none".to_string(),
            platform_tag,
        ));
    }
    // 5. no binary
    for minor in 0..=python_version.1 {
        tags.push((
            format!("py{}{}", python_version.0, minor),
            "none".to_string(),
            "any".to_string(),
        ));
    }
    tags.push((
        format!("py{}", python_version.0),
        "none".to_string(),
        "any".to_string(),
    ));
    tags.sort();
    Ok(tags)
}

/// All supported operating system
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Os {
    Manylinux { major: u16, minor: u16 },
    Musllinux { major: u16, minor: u16 },
    Windows,
    Macos { major: u16, minor: u16 },
    FreeBsd { release: String },
    NetBsd { release: String },
    OpenBsd { release: String },
    Dragonfly { release: String },
    Illumos { release: String, arch: String },
    Haiku { release: String },
}

impl Os {
    fn detect_linux_libc() -> anyhow::Result<Self> {
        let libc = find_libc()?;
        if let Ok(Some((major, minor))) = get_musl_version(&libc) {
            Ok(Os::Musllinux { major, minor })
        } else if let Ok(glibc_ld) = fs::read_link(&libc) {
            let filename = glibc_ld
                .file_name()
                .context("Expected the glibc ld to be a file")?
                .to_string_lossy();
            let expr = Regex::new(r"ld-(\d{1,3})\.(\d{1,3})\.so").unwrap();

            let capture = expr
                .captures(&filename)
                .with_context(|| format!("Invalid glibc ld filename: {}", filename))?;
            let major = capture.get(1).unwrap().as_str().parse::<u16>().unwrap();
            let minor = capture.get(2).unwrap().as_str().parse::<u16>().unwrap();
            Ok(Os::Manylinux { major, minor })
        } else {
            Err(anyhow!("Couldn't detect neither glibc version nor musl libc version, at least one of which is required"))
        }
    }

    pub fn current() -> std::result::Result<Self, WheelInstallerError> {
        let target_triple = target_lexicon::HOST;

        let os = match target_triple.operating_system {
            target_lexicon::OperatingSystem::Linux => {
                Self::detect_linux_libc().map_err(WheelInstallerError::OsVersionDetectionError)?
            }
            target_lexicon::OperatingSystem::Windows => Os::Windows,
            target_lexicon::OperatingSystem::MacOSX { major, minor, .. } => {
                Os::Macos { major, minor }
            }
            target_lexicon::OperatingSystem::Darwin => {
                let (major, minor) = get_mac_os_version()?;
                Os::Macos { major, minor }
            }
            target_lexicon::OperatingSystem::Netbsd => Os::NetBsd {
                release: PlatformInfo::new()?.release().to_string(),
            },
            target_lexicon::OperatingSystem::Freebsd => Os::FreeBsd {
                release: PlatformInfo::new()?.release().to_string(),
            },
            target_lexicon::OperatingSystem::Openbsd => Os::OpenBsd {
                release: PlatformInfo::new()?.release().to_string(),
            },
            target_lexicon::OperatingSystem::Dragonfly => Os::Dragonfly {
                release: PlatformInfo::new()?.release().to_string(),
            },
            target_lexicon::OperatingSystem::Illumos => {
                let platform_info = PlatformInfo::new()?;
                Os::Illumos {
                    release: platform_info.release().to_string(),
                    arch: platform_info.machine().to_string(),
                }
            }
            target_lexicon::OperatingSystem::Haiku => Os::Haiku {
                release: PlatformInfo::new()?.release().to_string(),
            },
            unsupported => {
                return Err(WheelInstallerError::OsVersionDetectionError(anyhow!(
                    "The operating system {:?} is not supported",
                    unsupported
                )))
            }
        };
        Ok(os)
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Os::Manylinux { .. } => write!(f, "Manylinux"),
            Os::Musllinux { .. } => write!(f, "Musllinux"),
            Os::Windows => write!(f, "Windows"),
            Os::Macos { .. } => write!(f, "MacOS"),
            Os::FreeBsd { .. } => write!(f, "FreeBSD"),
            Os::NetBsd { .. } => write!(f, "NetBSD"),
            Os::OpenBsd { .. } => write!(f, "OpenBSD"),
            Os::Dragonfly { .. } => write!(f, "DragonFly"),
            Os::Illumos { .. } => write!(f, "Illumos"),
            Os::Haiku { .. } => write!(f, "Haiku"),
        }
    }
}

/// All supported CPU architectures
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Arch {
    Aarch64,
    Armv7L,
    Powerpc64Le,
    Powerpc64,
    X86,
    X86_64,
    S390X,
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Arch::Aarch64 => write!(f, "aarch64"),
            Arch::Armv7L => write!(f, "armv7l"),
            Arch::Powerpc64Le => write!(f, "ppc64le"),
            Arch::Powerpc64 => write!(f, "ppc64"),
            Arch::X86 => write!(f, "i686"),
            Arch::X86_64 => write!(f, "x86_64"),
            Arch::S390X => write!(f, "s390x"),
        }
    }
}

impl Arch {
    pub fn current() -> Result<Arch, WheelInstallerError> {
        let target_triple = target_lexicon::HOST;
        let arch = match target_triple.architecture {
            target_lexicon::Architecture::X86_64 => Arch::X86_64,
            target_lexicon::Architecture::X86_32(_) => Arch::X86,
            target_lexicon::Architecture::Arm(_) => Arch::Armv7L,
            target_lexicon::Architecture::Aarch64(_) => Arch::Aarch64,
            target_lexicon::Architecture::Powerpc64 => Arch::Powerpc64,
            target_lexicon::Architecture::Powerpc64le => Arch::Powerpc64Le,
            target_lexicon::Architecture::S390x => Arch::S390X,
            unsupported => {
                return Err(WheelInstallerError::OsVersionDetectionError(anyhow!(
                    "The architecture {} is not supported",
                    unsupported
                )));
            }
        };
        Ok(arch)
    }

    /// Returns the oldest possible Manylinux tag for this architecture
    pub fn get_minimum_manylinux_minor(&self) -> u16 {
        match self {
            // manylinux 2014
            Arch::Aarch64 | Arch::Armv7L | Arch::Powerpc64 | Arch::Powerpc64Le | Arch::S390X => 17,
            // manylinux 1
            Arch::X86 | Arch::X86_64 => 5,
        }
    }
}

fn get_mac_os_version() -> Result<(u16, u16), WheelInstallerError> {
    // This is actually what python does
    // https://github.com/python/cpython/blob/cb2b3c8d3566ae46b3b8d0718019e1c98484589e/Lib/platform.py#L409-L428
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct SystemVersion {
        product_version: String,
    }
    let system_version: SystemVersion =
        plist::from_file("/System/Library/CoreServices/SystemVersion.plist")
            .map_err(|err| WheelInstallerError::OsVersionDetectionError(err.into()))?;

    let invalid_mac_os_version = || {
        WheelInstallerError::OsVersionDetectionError(anyhow!(
            "Invalid mac os version {}",
            system_version.product_version
        ))
    };
    match system_version
        .product_version
        .split('.')
        .collect::<Vec<&str>>()
        .as_slice()
    {
        [major, minor] | [major, minor, _] => {
            let major = major.parse::<u16>().map_err(|_| invalid_mac_os_version())?;
            let minor = minor.parse::<u16>().map_err(|_| invalid_mac_os_version())?;
            Ok((major, minor))
        }
        _ => Err(invalid_mac_os_version()),
    }
}

/// Find musl libc path from executable's ELF header
pub fn find_libc() -> anyhow::Result<PathBuf> {
    let buffer =
        fs::read("/bin/ls").context("Couldn't read /bin/ls for detecting the ld version")?;
    let parse_error = "Couldn't parse /bin/ls for detecting the ld version";
    let elf = Elf::parse(&buffer).context(parse_error)?;
    elf.interpreter.map(PathBuf::from).context(parse_error)
}

/// Read the musl version from libc library's output. Taken from maturin
///
/// The libc library should output something like this to stderr::
///
/// musl libc (x86_64)
/// Version 1.2.2
/// Dynamic Program Loader
pub fn get_musl_version(ld_path: impl AsRef<Path>) -> std::io::Result<Option<(u16, u16)>> {
    let output = Command::new(&ld_path.as_ref())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expr = Regex::new(r"Version (\d{2,4})\.(\d{2,4})").unwrap();
    if let Some(capture) = expr.captures(&stderr) {
        let major = capture.get(1).unwrap().as_str().parse::<u16>().unwrap();
        let minor = capture.get(2).unwrap().as_str().parse::<u16>().unwrap();
        return Ok(Some((major, minor)));
    }
    Ok(None)
}

/// Returns the compatible platform tags, e.g. manylinux_2_17, macosx_11_0_arm64 or win_amd64
///
/// We have two cases: Actual platform specific tags (including "merged" tags such as universal2)
/// and "any".
///
/// Bit of a mess, needs to be cleaned up
pub(crate) fn compatible_platform_tags(
    os: &Os,
    arch: &Arch,
) -> Result<Vec<String>, WheelInstallerError> {
    let platform_tags = match (os.clone(), *arch) {
        (Os::Manylinux { major, minor }, _) => {
            let mut platform_tags = vec![format!("linux_{}", arch)];
            platform_tags.extend(
                (arch.get_minimum_manylinux_minor()..=minor)
                    .map(|minor| format!("manylinux_{}_{}_{}", major, minor, arch)),
            );
            if (arch.get_minimum_manylinux_minor()..=minor).contains(&12) {
                platform_tags.push(format!("manylinux2010_{}", arch))
            }
            if (arch.get_minimum_manylinux_minor()..=minor).contains(&17) {
                platform_tags.push(format!("manylinux2014_{}", arch))
            }
            if (arch.get_minimum_manylinux_minor()..=minor).contains(&5) {
                platform_tags.push(format!("manylinux1_{}", arch))
            }
            platform_tags
        }
        (Os::Musllinux { major, minor }, _) => {
            let mut platform_tags = vec![format!("linux_{}", arch)];
            // musl 1.1 is the lowest supported version in musllinux
            platform_tags
                .extend((1..=minor).map(|minor| format!("musllinux_{}_{}_{}", major, minor, arch)));
            platform_tags
        }
        (Os::Macos { major, minor }, Arch::X86_64) => {
            assert!(major == 10 || major == 11);
            let mut platform_tags = vec![];
            match major {
                10 => {
                    platform_tags.extend(
                        (0..=minor).map(|minor| format!("macosx_{}_{}_x86_64", major, minor)),
                    );
                    platform_tags.extend(
                        (0..=minor).map(|minor| format!("macosx_{}_{}_universal2", major, minor)),
                    );
                }
                11 => {
                    platform_tags.extend(
                        (0..=minor).map(|minor| format!("macosx_{}_{}_x86_64", major, minor)),
                    );
                    platform_tags.extend(
                        (0..=minor).map(|minor| format!("macosx_{}_{}_universal2", major, minor)),
                    );
                    // Mac os 10 backwards compatibility
                    platform_tags
                        .extend((0..=15).map(|minor| format!("macosx_{}_{}_x86_64", 10, minor)));
                    platform_tags.extend(
                        (0..=15).map(|minor| format!("macosx_{}_{}_universal2", 10, minor)),
                    );
                }
                _ => {
                    return Err(WheelInstallerError::OsVersionDetectionError(anyhow!(
                        "Unsupported mac os version: {}",
                        major,
                    )));
                }
            }
            platform_tags
        }
        (Os::Macos { major, minor }, Arch::Aarch64) => {
            // arm64 (aka apple silicon) needs mac os 11
            assert_eq!(major, 11);
            let mut platform_tags = vec![];
            platform_tags
                .extend((0..=minor).map(|minor| format!("macosx_{}_{}_arm64", major, minor)));
            platform_tags
                .extend((0..=minor).map(|minor| format!("macosx_{}_{}_universal2", major, minor)));
            platform_tags
        }
        (Os::Windows, Arch::X86) => {
            vec!["win32".to_string()]
        }
        (Os::Windows, Arch::X86_64) => {
            vec!["win_amd64".to_string()]
        }
        (Os::Windows, Arch::Aarch64) => vec!["win_arm64".to_string()],
        (
            Os::FreeBsd { release: _ }
            | Os::NetBsd { release: _ }
            | Os::OpenBsd { release: _ }
            | Os::Dragonfly { release: _ }
            | Os::Haiku { release: _ },
            _,
        ) => {
            let info = PlatformInfo::new()?;
            let release = info.release().replace(".", "_").replace("-", "_");
            vec![format!(
                "{}_{}_{}",
                os.to_string().to_lowercase(),
                release,
                arch
            )]
        }
        (
            Os::Illumos {
                mut release,
                mut arch,
            },
            _,
        ) => {
            let mut os = os.to_string().to_lowercase();
            // See https://github.com/python/cpython/blob/46c8d915715aa2bd4d697482aa051fe974d440e1/Lib/sysconfig.py#L722-L730
            if let Some((major, other)) = release.split_once('_') {
                let major_ver: u64 = major
                    .parse()
                    .context("illumos major version is not a number")
                    .map_err(WheelInstallerError::OsVersionDetectionError)?;
                if major_ver >= 5 {
                    // SunOS 5 == Solaris 2
                    os = "solaris".to_string();
                    release = format!("{}_{}", major_ver - 3, other);
                    arch = format!("{}_64bit", arch);
                }
            }
            vec![format!("{}_{}_{}", os, release, arch)]
        }
        _ => {
            return Err(WheelInstallerError::OsVersionDetectionError(anyhow!(
                "Unsupported operating system and architecture combination: {} {}",
                os,
                arch
            )));
        }
    };
    Ok(platform_tags)
}

#[cfg(test)]
mod test {
    use crate::wheel_tags::{compatible_platform_tags, compatible_tags, Arch, Os, WheelFilename};
    use crate::WheelInstallerError;
    use fs_err::File;
    use std::str::FromStr;

    const FILENAMES: &[&str] = &[
        "numpy-1.22.2-pp38-pypy38_pp73-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "numpy-1.22.2-cp310-cp310-win_amd64.whl",
        "numpy-1.22.2-cp310-cp310-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "numpy-1.22.2-cp310-cp310-manylinux_2_17_aarch64.manylinux2014_aarch64.whl",
        "numpy-1.22.2-cp310-cp310-macosx_11_0_arm64.whl",
        "numpy-1.22.2-cp310-cp310-macosx_10_14_x86_64.whl",
        "numpy-1.22.2-cp39-cp39-win_amd64.whl",
        "numpy-1.22.2-cp39-cp39-win32.whl",
        "numpy-1.22.2-cp39-cp39-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "numpy-1.22.2-cp39-cp39-manylinux_2_17_aarch64.manylinux2014_aarch64.whl",
        "numpy-1.22.2-cp39-cp39-macosx_11_0_arm64.whl",
        "numpy-1.22.2-cp39-cp39-macosx_10_14_x86_64.whl",
        "numpy-1.22.2-cp38-cp38-win_amd64.whl",
        "numpy-1.22.2-cp38-cp38-win32.whl",
        "numpy-1.22.2-cp38-cp38-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "numpy-1.22.2-cp38-cp38-manylinux_2_17_aarch64.manylinux2014_aarch64.whl",
        "numpy-1.22.2-cp38-cp38-macosx_11_0_arm64.whl",
        "numpy-1.22.2-cp38-cp38-macosx_10_14_x86_64.whl",
        "tqdm-4.62.3-py2.py3-none-any.whl",
    ];

    /// Test that we can parse the filenames
    #[test]
    fn test_wheel_filename_parsing() -> Result<(), WheelInstallerError> {
        for filename in FILENAMES {
            WheelFilename::from_str(filename)?;
        }
        Ok(())
    }

    /// Test that we correctly identify compatible pairs
    #[test]
    fn test_compatibility() -> anyhow::Result<()> {
        let filenames = [
            (
                "numpy-1.22.2-cp38-cp38-win_amd64.whl",
                ((3, 8), Os::Windows, Arch::X86_64),
            ),
            (
                "numpy-1.22.2-cp38-cp38-win32.whl",
                ((3, 8), Os::Windows, Arch::X86),
            ),
            (
                "numpy-1.22.2-cp38-cp38-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
                (
                    (3, 8),
                    Os::Manylinux {
                        major: 2,
                        minor: 31,
                    },
                    Arch::X86_64,
                ),
            ),
            (
                "numpy-1.22.2-cp38-cp38-manylinux_2_17_aarch64.manylinux2014_aarch64.whl",
                (
                    (3, 8),
                    Os::Manylinux {
                        major: 2,
                        minor: 31,
                    },
                    Arch::Aarch64,
                ),
            ),
            (
                "numpy-1.22.2-cp38-cp38-macosx_11_0_arm64.whl",
                (
                    (3, 8),
                    Os::Macos {
                        major: 11,
                        minor: 0,
                    },
                    Arch::Aarch64,
                ),
            ),
            (
                "numpy-1.22.2-cp38-cp38-macosx_10_14_x86_64.whl",
                (
                    (3, 8),
                    // Test backwards compatibility here
                    Os::Macos {
                        major: 11,
                        minor: 0,
                    },
                    Arch::X86_64,
                ),
            ),
            (
                "tqdm-4.62.3-py2.py3-none-any.whl",
                (
                    (3, 8),
                    Os::Manylinux {
                        major: 2,
                        minor: 31,
                    },
                    Arch::X86_64,
                ),
            ),
        ];

        for (filename, (python_version, os, arch)) in filenames {
            let compatible_tags = compatible_tags(python_version, &os, &arch)?;
            assert!(
                WheelFilename::from_str(filename)?.is_compatible(&compatible_tags),
                "{}",
                filename
            );
        }
        Ok(())
    }

    /// Test that incompatible pairs don't pass is_compatible
    #[test]
    fn test_compatibility_filter() -> anyhow::Result<()> {
        let compatible_tags = compatible_tags(
            (3, 8),
            &Os::Manylinux {
                major: 2,
                minor: 31,
            },
            &Arch::X86_64,
        )?;

        let compatible: Vec<&str> = FILENAMES
            .iter()
            .filter(|filename| {
                WheelFilename::from_str(filename)
                    .unwrap()
                    .is_compatible(&compatible_tags)
            })
            .cloned()
            .collect();
        assert_eq!(
            vec![
                "numpy-1.22.2-cp38-cp38-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
                "tqdm-4.62.3-py2.py3-none-any.whl"
            ],
            compatible
        );
        Ok(())
    }

    fn get_ubuntu_20_04_tags() -> anyhow::Result<Vec<String>> {
        Ok(serde_json::from_reader(File::open(
            "test-data/tags/cp38-ubuntu-20-04.json",
        )?)?)
    }

    /// Check against the tags that packaging.tags reports as compatible
    #[test]
    fn ubuntu_20_04_compatible() -> anyhow::Result<()> {
        let tags = get_ubuntu_20_04_tags()?;
        for tag in tags {
            let compatible_tags = compatible_tags(
                (3, 8),
                &Os::Manylinux {
                    major: 2,
                    minor: 31,
                },
                &Arch::X86_64,
            )?;

            assert!(
                WheelFilename::from_str(&format!("foo-1.0-{}.whl", tag))?
                    .is_compatible(&compatible_tags),
                "{}",
                tag
            )
        }
        Ok(())
    }

    /// Check against the tags that packaging.tags reports as compatible
    #[test]
    fn ubuntu_20_04_list() -> anyhow::Result<()> {
        let expected_tags = get_ubuntu_20_04_tags()?;
        let actual_tags: Vec<String> = compatible_tags(
            (3, 8),
            &Os::Manylinux {
                major: 2,
                minor: 31,
            },
            &Arch::X86_64,
        )?
        .iter()
        .map(|(python_tag, abi_tag, platform_tag)| {
            format!("{}-{}-{}", python_tag, abi_tag, platform_tag)
        })
        .collect();
        assert_eq!(expected_tags, actual_tags);
        Ok(())
    }

    /// Basic does-it-work test
    #[test]
    fn host_arch() -> anyhow::Result<()> {
        let os = Os::current()?;
        let arch = Arch::current()?;
        compatible_platform_tags(&os, &arch)?;
        Ok(())
    }
}
