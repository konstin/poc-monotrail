use crate::spec::RequestedSpec;
use anyhow::bail;
use regex::Regex;

/// Reads a simple requirements.txt format (only name and optionally version),
/// returns a spec with name and optionally version
pub fn requirements_txt_to_specs(requirements_txt: &str) -> anyhow::Result<Vec<RequestedSpec>> {
    let re = Regex::new(r"^(?P<name>[\w\d_\-]+)(\s*==\s*(?P<version>[\d\w.\-]+))?$").unwrap();
    requirements_txt
        .lines()
        .enumerate()
        .map(|(pos, line)| (pos, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .map(|(pos, line)| match re.captures(line) {
            None => {
                // +1 to correct for zero indexing
                bail!(
                    "invalid version specification in line {}: '{}'",
                    pos + 1,
                    line
                )
            }
            Some(captures) => Ok(RequestedSpec {
                requested: line.to_string(),
                name: captures.name("name").unwrap().as_str().to_string(),
                python_version: captures
                    .name("version")
                    .map(|version| version.as_str().to_string()),
                source: None,
                extras: vec![],
                file_path: None,
                url: None,
            }),
        })
        .collect()
}

#[cfg(test)]
mod test {
    use crate::requirements_txt::requirements_txt_to_specs;
    use crate::spec::RequestedSpec;
    use indoc::indoc;

    #[test]
    fn test_requirements_txt() {
        let valid = indoc! {"

            inflection==0.5.1
            upsidedown==0.4
            numpy

        "};

        let expected = vec![
            RequestedSpec {
                requested: "inflection==0.5.1".to_string(),
                name: "inflection".to_string(),
                python_version: Some("0.5.1".to_string()),
                source: None,
                extras: vec![],
                file_path: None,
                url: None,
            },
            RequestedSpec {
                requested: "upsidedown==0.4".to_string(),
                name: "upsidedown".to_string(),
                python_version: Some("0.4".to_string()),
                source: None,
                extras: vec![],
                file_path: None,
                url: None,
            },
            RequestedSpec {
                requested: "numpy".to_string(),
                name: "numpy".to_string(),
                python_version: None,
                source: None,
                extras: vec![],
                file_path: None,
                url: None,
            },
        ];

        assert_eq!(requirements_txt_to_specs(valid).unwrap(), expected);
    }

    #[test]
    fn test_requirements_txt_error() {
        let invalid = indoc! {"

            inflection==0.5.1
            upsidedown=0.4

        "};

        assert_eq!(
            requirements_txt_to_specs(invalid).unwrap_err().to_string(),
            "invalid version specification in line 3: 'upsidedown=0.4'"
        );
    }
}
