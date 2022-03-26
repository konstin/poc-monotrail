//! I have no idea how to write parsers and didn't want to learn it,
//! so if you want to replace this with something proper feel free to :)
//! I really wouldn't have written this in the first place if it wasn't absolutely required
//!
//! https://peps.python.org/pep-0508/#grammar
//! https://github.com/pypa/pip/blob/b4d2b0f63f4955c7d6eee2653c6e1fa6fa507c31/src/pip/_vendor/distlib/markers.py

#![allow(dead_code)]

use serde::Deserialize;
use std::fmt::{Display, Formatter};
use std::io;
use std::process::{Command, Stdio};
use std::str::FromStr;

static PEP508_QUERY_SCRIPT: &str = include_str!("get_pep508_env.py");

#[derive(Debug, Eq, PartialEq, Deserialize)]
struct PythonEnvironment {
    implementation_name: String,
    implementation_version: String,
    os_name: String,
    platform_machine: String,
    platform_python_implementation: String,
    platform_release: String,
    platform_system: String,
    platform_version: String,
    python_full_version: String,
    python_version: String,
    sys_platform: String,
}

impl PythonEnvironment {
    fn new() -> Self {
        PythonEnvironment {
            implementation_name: "".to_string(),
            implementation_version: "".to_string(),
            os_name: "".to_string(),
            platform_machine: "".to_string(),
            platform_python_implementation: "".to_string(),
            platform_release: "".to_string(),
            platform_system: "".to_string(),
            platform_version: "".to_string(),
            python_full_version: "".to_string(),
            python_version: "".to_string(),
            sys_platform: "".to_string(),
        }
    }

    fn get_key(&self, key: Key) -> &str {
        match key {
            Key::ImplementationName => &self.implementation_name,
            Key::ImplementationVersion => &self.implementation_version,
            Key::OsName => &self.os_name,
            Key::PlatformMachine => &self.platform_machine,
            Key::PlatformPythonImplementation => &self.platform_python_implementation,
            Key::PlatformRelease => &self.platform_release,
            Key::PlatformSystem => &self.platform_system,
            Key::PlatformVersion => &self.platform_version,
            Key::PythonFullVersion => &self.python_full_version,
            Key::PythonVersion => &self.python_version,
            Key::SysPlatform => &self.sys_platform,
        }
    }

    /// Runs python to get the actual PEP 508 value
    fn from_python() -> Self {
        let out = Command::new("python")
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .expect("piped stdin")
                    .write_all(PEP508_QUERY_SCRIPT.as_bytes())?;
                child.wait_with_output()
            });

        let returned = match out {
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    panic!(
                        "Could not find any interpreter at {}, \
                        are you sure you have Python installed on your PATH?",
                        "python"
                    )
                } else {
                    panic!(
                        "Failed to run the Python interpreter at {}: {}",
                        "python", err
                    )
                }
            }
            Ok(ok) if !ok.status.success() => panic!("Python script failed"),
            Ok(ok) => ok.stdout,
        };
        serde_json::from_slice(&returned).unwrap()
    }
}

fn position_with_start(chars: &[char], start: usize, cond: impl Fn(char) -> bool) -> usize {
    for pos in start..chars.len() {
        if !cond(chars[pos]) {
            return pos;
        }
    }
    return chars.len();
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum Key {
    ImplementationName,
    ImplementationVersion,
    OsName,
    PlatformMachine,
    PlatformPythonImplementation,
    PlatformRelease,
    PlatformSystem,
    PlatformVersion,
    PythonFullVersion,
    PythonVersion,
    SysPlatform,
}

impl FromStr for Key {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = match s {
            "implementation_name" => Self::ImplementationName,
            "implementation_version" => Self::ImplementationVersion,
            "os_name" => Self::OsName,
            "platform_machine" => Self::PlatformMachine,
            "platform_python_implementation" => Self::PlatformPythonImplementation,
            "platform_release" => Self::PlatformRelease,
            "platform_system" => Self::PlatformSystem,
            "platform_version" => Self::PlatformVersion,
            "python_full_version" => Self::PythonFullVersion,
            "python_version" => Self::PythonVersion,
            "sys_platform" => Self::SysPlatform,
            _ => return Err(()),
        };
        return Ok(value);
    }
}
impl Display for Key {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ImplementationName => "implementation_name",
            Self::ImplementationVersion => "implementation_version",
            Self::OsName => "os_name",
            Self::PlatformMachine => "platform_machine",
            Self::PlatformPythonImplementation => "platform_python_implementation",
            Self::PlatformRelease => "platform_release",
            Self::PlatformSystem => "platform_system",
            Self::PlatformVersion => "platform_version",
            Self::PythonFullVersion => "python_full_version",
            Self::PythonVersion => "python_version",
            Self::SysPlatform => "sys_platform",
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Comparator {
    Equal,
    Larger,
    LargerEqual,
    Smaller,
    SmallerEqual,
    In,
    NotIn,
}

impl FromStr for Comparator {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = match s {
            "==" => Self::Equal,
            ">" => Self::Larger,
            ">=" => Self::LargerEqual,
            "<" => Self::Smaller,
            "<=" => Self::SmallerEqual,
            "in" => Self::In,
            "not in" => Self::NotIn,
            _ => return Err(()),
        };
        Ok(value)
    }
}

impl Display for Comparator {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Equal => "==",
            Self::Larger => ">",
            Self::LargerEqual => ">=",
            Self::Smaller => "<",
            Self::SmallerEqual => "<=",
            Self::In => "in",
            Self::NotIn => "not in",
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Expression {
    key: Key,
    comparator: Comparator,
    value: String,
}

impl Expression {
    fn evaluate(&self, env: &PythonEnvironment) -> bool {
        match self.key {
            Key::OsName
            | Key::SysPlatform
            | Key::PlatformPythonImplementation
            | Key::PlatformRelease
            | Key::PlatformVersion
            | Key::ImplementationName => match self.comparator {
                Comparator::Equal => env.get_key(self.key) == self.value,
                Comparator::In => self.value.contains(env.get_key(self.key)),
                Comparator::NotIn => !self.value.contains(env.get_key(self.key)),
                _ => panic!(
                    "comparator {} not supported for {}",
                    self.comparator, self.key
                ),
            },
            _ => match self.comparator {
                Comparator::Equal => env.get_key(self.key) == self.value,
                Comparator::In => self.value.contains(env.get_key(self.key)),
                Comparator::NotIn => !self.value.contains(env.get_key(self.key)),
                _ => panic!(
                    "comparator {} not supported for {}",
                    self.comparator, self.key
                ),
            },
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ExpressionTree {
    Expression(Expression),
    And(Vec<ExpressionTree>),
    Or(Vec<ExpressionTree>),
}

impl ExpressionTree {
    fn evaluate(&self, env: &PythonEnvironment) -> bool {
        match self {
            ExpressionTree::Expression(expression) => expression.evaluate(env),
            ExpressionTree::And(and_list) => and_list
                .iter()
                .map(|expression| expression.evaluate(env))
                .all(|x| x),
            ExpressionTree::Or(or_list) => or_list
                .iter()
                .map(|expression| expression.evaluate(env))
                .any(|x| x),
        }
    }
}

fn parse_marker(marker: &str) -> Result<ExpressionTree, ()> {
    // 😈
    // behold my horror of the parser
    let chars: Vec<char> = marker.chars().collect();

    let (expression, end_expression) = parse_expression_list(&chars, 0)?;
    if end_expression < chars.len() {
        panic!("{} {}", end_expression, chars.len());
    }
    Ok(expression)
}

fn parse_expression_list(
    chars: &[char],
    start_expression: usize,
) -> Result<(ExpressionTree, usize), ()> {
    // "and" has precedence over "or", so we make an "or" list that consists of single or a list of "and" nodes
    let mut or_list: Vec<ExpressionTree> = Vec::new();
    let mut and_list = Vec::new();

    let (expression, end_expression) = if chars[start_expression] == '(' {
        parse_expression_list(&chars, start_expression + 1)?
    } else {
        let (expression, end_expression) = parse_expression(&chars, start_expression)?;
        (ExpressionTree::Expression(expression), end_expression)
    };
    and_list.push(expression);
    let mut start_and_or = position_with_start(&chars, end_expression, |c| c.is_whitespace());
    while start_and_or < chars.len() && chars[start_and_or] != ')' {
        let end_and_or = position_with_start(&chars, start_and_or, |c| !c.is_whitespace());
        let start_expression = position_with_start(&chars, end_and_or, |c| c.is_whitespace());

        let conjunction_and = match chars[start_and_or..end_and_or]
            .iter()
            .collect::<String>()
            .as_str()
        {
            "and" => true,
            "or" => false,
            other => panic!(
                "invalid conjunction: {} {} {}",
                other, start_and_or, end_and_or,
            ),
        };

        let (expression, end_expression) = if chars[start_expression] == '(' {
            parse_expression_list(&chars, start_expression + 1)?
        } else {
            let (expression, end_expression) = parse_expression(&chars, start_expression)?;
            (ExpressionTree::Expression(expression), end_expression)
        };

        if conjunction_and {
            and_list.push(expression);
        } else {
            // we do a lot of these shenanigans so we get simple equality checks for free later
            or_list.push(if and_list.len() == 1 {
                and_list.swap_remove(0)
            } else {
                ExpressionTree::And(and_list)
            });
            and_list = vec![expression];
        }

        start_and_or = position_with_start(&chars, end_expression, |c| c.is_whitespace());
    }
    or_list.push(if and_list.len() == 1 {
        and_list.swap_remove(0)
    } else {
        ExpressionTree::And(and_list)
    });
    let tree = if or_list.len() == 1 {
        or_list.swap_remove(0)
    } else {
        ExpressionTree::Or(or_list)
    };
    if start_and_or < chars.len() {
        // skip closing brace
        Ok((tree, start_and_or + 1))
    } else {
        Ok((tree, start_and_or))
    }
}

/// parses <keyword> <comparator> <value>, e.g. `python_version == '2.7'`
fn parse_expression(chars: &[char], start_name: usize) -> Result<(Expression, usize), ()> {
    let end_name = position_with_start(&chars, start_name, |c| {
        c.is_ascii_alphanumeric() || c == '_'
    });
    let name = &chars[start_name..end_name];
    let key = Key::from_str(&name.iter().collect::<String>())?;

    let start_comparator = position_with_start(&chars, end_name, |c| c.is_whitespace());
    let end_comparator = position_with_start(&chars, start_comparator, |c| {
        !c.is_whitespace() && c != '\'' && c != '"'
    });
    let comparator = &chars[start_comparator..end_comparator];
    let comparator = Comparator::from_str(&comparator.iter().collect::<String>())?;

    let start_value_quote = position_with_start(&chars, end_comparator, |c| c.is_whitespace());
    // haha yes
    let quote_char = chars[start_value_quote];

    let end_value = position_with_start(&chars, start_value_quote + 1, |c| c != quote_char);
    let value = &chars[start_value_quote + 1..end_value];
    let value = value.iter().collect::<String>();

    let expression = Expression {
        key,
        comparator,
        value,
    };
    let end_value_quote = end_value + 1;
    Ok((expression, end_value_quote))
}

#[cfg(test)]
mod test {
    use crate::markers::{parse_marker, PythonEnvironment};

    #[test]
    fn get_python() {
        PythonEnvironment::from_python();
    }

    #[test]
    fn asdf() {
        let env27 = PythonEnvironment {
            implementation_name: "".to_string(),
            implementation_version: "".to_string(),
            os_name: "linux".to_string(),
            platform_machine: "".to_string(),
            platform_python_implementation: "".to_string(),
            platform_release: "".to_string(),
            platform_system: "".to_string(),
            platform_version: "".to_string(),
            python_full_version: "".to_string(),
            python_version: "2.7".to_string(),
            sys_platform: "linux".to_string(),
        };
        let env37 = PythonEnvironment {
            implementation_name: "".to_string(),
            implementation_version: "".to_string(),
            os_name: "linux".to_string(),
            platform_machine: "".to_string(),
            platform_python_implementation: "".to_string(),
            platform_release: "".to_string(),
            platform_system: "".to_string(),
            platform_version: "".to_string(),
            python_full_version: "".to_string(),
            python_version: "3.7".to_string(),
            sys_platform: "linux".to_string(),
        };
        let marker1 = parse_marker("python_version == '2.7'").unwrap();
        let marker2 = parse_marker(
            "os_name == \"linux\" or python_version == \"3.7\" and sys_platform == \"win32\"",
        )
        .unwrap();
        let marker3 = parse_marker(
                "python_version == \"2.7\" and (sys_platform == \"win32\" or sys_platform == \"linux\")",
        ).unwrap();
        assert!(marker1.evaluate(&env27));
        assert!(!marker1.evaluate(&env37));
        dbg!(marker2.evaluate(&env27), marker3.evaluate(&env27));
        dbg!(marker2.evaluate(&env37), marker3.evaluate(&env37));
    }

    #[test]
    fn test_marker_matches() {
        let values = [
            ("python_version == '2.7'", "python_version == \"2.7\""),
            ("python_version == \"2.7\"", "python_version == \"2.7\""),
            (
                "python_version == \"2.7\" and os_name == \"linux\"",
                "python_version == \"2.7\" and os_name == \"linux\"",
            ),
            (
                "python_version == \"2.7\" or os_name == \"linux\"",
                "python_version == \"2.7\" or os_name == \"linux\"",
            ),
            (
                "python_version == \"2.7\" and os_name == \"linux\" or sys_platform == \"win32\"",
                "python_version == \"2.7\" and os_name == \"linux\" or sys_platform == \"win32\"",
            ),
            ("(python_version == \"2.7\")", "python_version == \"2.7\""),
            (
                "(python_version == \"2.7\" and sys_platform == \"win32\")",
                "python_version == \"2.7\" and sys_platform == \"win32\"",
            ),
            (
                "python_version == \"2.7\" and (sys_platform == \"win32\" or sys_platform == \"linux\")",
                "python_version == \"2.7\" and (sys_platform == \"win32\" or sys_platform == \"linux\")",
            ),
        ];
        for (a, b) in values {
            assert_eq!(parse_marker(a).unwrap(), parse_marker(b).unwrap());
        }
    }
}
