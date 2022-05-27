use crate::poetry_integration::poetry_toml;
use anyhow::{bail, format_err, Context};
use requirements::enums::Comparison;
use std::collections::HashMap;
use std::path::Path;

/// Reads requirements.txt badly with lots of features unsupported and others wrongly implemented
pub fn parse_requirements_txt(
    requirements: &str,
    // as debug info
    requirements_txt: &Path,
) -> anyhow::Result<HashMap<String, poetry_toml::Dependency>> {
    let requirements = requirements::parse_str(&requirements)
        .map_err(|err| format_err!("Failed to parse {}: {}", requirements_txt.display(), err))?;

    let mut poetry_requirements: HashMap<String, poetry_toml::Dependency> = HashMap::new();
    for requirement in requirements {
        if requirement.vcs.is_some()
            || requirement.uri.is_some()
            || requirement.subdirectory.is_some()
            || requirement.editable
            || !requirement.extra_index_url.is_empty()
        {
            bail!(
                "Unsupported feature in '{}' of {}",
                requirement.line,
                requirements_txt.display()
            );
        }
        assert!(requirement.specifier);
        let git: Option<String> = if let Some(_) = requirement.vcs {
            bail!("Not implemented")
        } else {
            None
        };

        let version = if !requirement.specs.is_empty() {
            let specs = requirement
                .specs
                .iter()
                .map(|(comparison, version)| {
                    if comparison == &Comparison::Equal {
                        version.to_string()
                    } else {
                        format!("{}{}", comparison, version)
                    }
                })
                .collect::<Vec<String>>();
            Some(specs.join(","))
        } else if git.is_some() {
            None
        } else {
            Some("*".to_string())
        };

        let dep = poetry_toml::Dependency::Expanded {
            version,
            optional: Some(false),
            extras: Some(requirement.extras.clone()),
            git,
            branch: requirement.revision.clone(),
        };

        let name = requirement.name.with_context(|| {
            format!(
                "requirement needs a name in '{}' of {}",
                requirement.line,
                requirements_txt.display()
            )
        })?;
        poetry_requirements.insert(name, dep);
    }
    Ok(poetry_requirements)
}

#[cfg(test)]
mod test {
    use crate::requirements_txt::parse_requirements_txt;
    use std::collections::BTreeMap;
    use std::path::Path;

    use indoc::indoc;

    #[test]
    fn test_requirements_txt() {
        let valid = indoc! {"

            inflection==0.5.1
            upsidedown==0.4
            numpy

        "};

        let expected = indoc! {"
            [inflection]
            version = \"0.5.1\"
            optional = false
            extras = []
            
            [numpy]
            version = \"*\"
            optional = false
            extras = []
            
            [upsidedown]
            version = \"0.4\"
            optional = false
            extras = []
        "};

        let reqs = parse_requirements_txt(valid, Path::new("")).unwrap();
        // sort lines
        let reqs = BTreeMap::from_iter(&reqs);
        let poetry_toml = toml::to_string(&reqs).unwrap();
        assert_eq!(poetry_toml, expected);
    }
}
