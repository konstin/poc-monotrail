use regex::Regex;

/// Reads a simple requirements.txt format (only name and optionally version),
/// returns a spec with name and optionally version
pub fn parse_requirements_txt(
    requirements_txt: &str,
) -> Result<Vec<(String, Option<String>)>, String> {
    let re = Regex::new(r"^(?P<name>[\w\d_\-]+)(\s*==\s*(?P<version>[\d\w.\-]+))?$").unwrap();
    requirements_txt
        .lines()
        .enumerate()
        .map(|(pos, line)| (pos, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .map(|(pos, line)| match re.captures(line) {
            None => {
                // +1 to correct for zero indexing
                return Err(format!(
                    "invalid version specification in line {}: '{}'",
                    pos + 1,
                    line
                ));
            }
            Some(captures) => Ok((
                captures.name("name").unwrap().as_str().to_string(),
                captures
                    .name("version")
                    .map(|version| version.as_str().to_string()),
            )),
        })
        .collect()
}

#[cfg(test)]
mod test {
    use crate::requirements_txt::parse_requirements_txt;

    use indoc::indoc;

    #[test]
    fn test_requirements_txt() {
        let valid = indoc! {"

            inflection==0.5.1
            upsidedown==0.4
            numpy

        "};

        let expected = vec![
            ("inflection".to_string(), Some("0.5.1".to_string())),
            ("upsidedown".to_string(), Some("0.4".to_string())),
            ("numpy".to_string(), None),
        ];

        assert_eq!(parse_requirements_txt(valid).unwrap(), expected);
    }

    #[test]
    fn test_requirements_txt_error() {
        let invalid = indoc! {"

            inflection==0.5.1
            upsidedown=0.4

        "};

        assert_eq!(
            parse_requirements_txt(invalid).unwrap_err(),
            "invalid version specification in line 3: 'upsidedown=0.4'"
        );
    }
}
