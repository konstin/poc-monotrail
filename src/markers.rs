//! PEP 508 marker parser
//!
//! i have no idea how to write parsers and didn't want to learn it just for this,
//! so if you want to replace this with something proper feel free to :)
//! i really wouldn't have written this in the first place if it wasn't absolutely required
//!
//! just to be clear: the parser below is horrible
//!
//! <https://peps.python.org/pep-0508/#grammar>
//! <https://github.com/pypa/pip/blob/b4d2b0f63f4955c7d6eee2653c6e1fa6fa507c31/src/pip/_vendor/distlib/markers.py>

use crate::PEP508_QUERY_ENV;
use pep440_rs::Version;
use serde::Deserialize;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;

/// The version and platform information required to evaluate marker expressions according to PEP 508
#[derive(Debug, Eq, PartialEq, Deserialize, Clone)]
pub struct Pep508Environment {
    pub(crate) implementation_name: String,
    pub(crate) implementation_version: String,
    pub(crate) os_name: String,
    pub(crate) platform_machine: String,
    pub(crate) platform_python_implementation: String,
    pub(crate) platform_release: String,
    pub(crate) platform_system: String,
    pub(crate) platform_version: String,
    pub(crate) python_full_version: String,
    pub(crate) python_version: String,
    pub(crate) sys_platform: String,
}

impl Pep508Environment {
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

    /// If we launch from python, we can call the python code from python with no overhead, but
    /// still need to parse into Self here
    #[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
    pub fn from_json_str(pep508_env_data: &str) -> Self {
        serde_json::from_str(pep508_env_data).unwrap()
    }

    /// Runs python to get the actual PEP 508 values
    ///
    /// To be eventually replaced by something like the maturin solution where we construct this
    /// is in rust
    pub fn from_python(python: &Path) -> Self {
        let out = Command::new(python)
            .args(["-S"])
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                // We only have the module definition in that file (because we also want to load
                // it as a module in the python bindings), so we need to append the actual call
                let pep508_query_script = format!("{}\nprint(get_pep508_env())", PEP508_QUERY_ENV);
                child
                    .stdin
                    .as_mut()
                    .expect("piped stdin")
                    .write_all(pep508_query_script.as_bytes())?;
                child.wait_with_output()
            });

        let returned = match out {
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    panic!(
                        "Could not find any interpreter at {}, \
                        are you sure you have Python installed on your PATH?",
                        python.display()
                    )
                } else {
                    panic!(
                        "Failed to run the Python interpreter at {}: {}",
                        python.display(),
                        err
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
    for (pos, char) in chars.iter().enumerate().skip(start) {
        if !cond(*char) {
            return pos;
        }
    }
    chars.len()
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
    type Err = String;

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
            _ => return Err(format!("Invalid key: {}", s)),
        };
        Ok(value)
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
    NotEqual,
    Larger,
    LargerEqual,
    Smaller,
    SmallerEqual,
    In,
    NotIn,
}

impl FromStr for Comparator {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = match s {
            "==" => Self::Equal,
            "!=" => Self::NotEqual,
            ">" => Self::Larger,
            ">=" => Self::LargerEqual,
            "<" => Self::Smaller,
            "<=" => Self::SmallerEqual,
            "in" => Self::In,
            "not in" => Self::NotIn,
            _ => return Err(format!("Invalid comparator: {}", s)),
        };
        Ok(value)
    }
}

impl Display for Comparator {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Larger => ">",
            Self::LargerEqual => ">=",
            Self::Smaller => "<",
            Self::SmallerEqual => "<=",
            Self::In => "in",
            Self::NotIn => "not in",
        })
    }
}

/// Represents one clause in the form <a name from the PEP508 list> <an operator> <a value>
#[derive(Debug, Eq, PartialEq)]
pub struct Expression {
    key: Key,
    comparator: Comparator,
    value: String,
}

impl Display for Expression {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let quoted_value = if self.value.contains('\'') {
            format!("\"{}\"", self.value)
        } else {
            format!("'{}'", self.value)
        };
        write!(f, "{} {} {}", self.key, self.comparator, quoted_value)
    }
}

impl Expression {
    /// Determines whether the expression is true or false in the given environment
    fn evaluate(&self, env: &Pep508Environment) -> Result<bool, String> {
        Ok(match self.key {
            Key::OsName
            | Key::SysPlatform
            | Key::PlatformPythonImplementation
            | Key::PlatformMachine
            | Key::PlatformRelease
            | Key::PlatformSystem
            | Key::PlatformVersion
            | Key::ImplementationName => match self.comparator {
                Comparator::Equal => env.get_key(self.key) == self.value,
                Comparator::NotEqual => env.get_key(self.key) != self.value,
                Comparator::In => self.value.contains(env.get_key(self.key)),
                Comparator::NotIn => !self.value.contains(env.get_key(self.key)),
                _ => {
                    return Err(format!(
                        "comparator {} not supported for {} (not a version)",
                        self.comparator, self.key
                    ))
                }
            },
            Key::ImplementationVersion | Key::PythonFullVersion | Key::PythonVersion => {
                let left_version = Version::from_str(env.get_key(self.key)).map_err(|err| {
                    format!("{} is not a valid pep440 version: {}", self.value, err)
                })?;
                let right_version = Version::from_str(&self.value).map_err(|err| {
                    format!("{} is not a valid pep440 version: {}", self.value, err)
                })?;
                match self.comparator {
                    // Should this actually use equality or should we compare with pep440
                    Comparator::Equal => env.get_key(self.key) == self.value,
                    Comparator::NotEqual => env.get_key(self.key) != self.value,
                    Comparator::In => self.value.contains(env.get_key(self.key)),
                    Comparator::NotIn => !self.value.contains(env.get_key(self.key)),
                    Comparator::Larger => left_version > right_version,
                    Comparator::LargerEqual => left_version >= right_version,
                    Comparator::Smaller => left_version < right_version,
                    Comparator::SmallerEqual => left_version <= right_version,
                }
            }
        })
    }
}

/// Represents one of the nested marker expressions with and/or/parentheses
#[derive(Debug, Eq, PartialEq)]
pub enum ExpressionTree {
    Expression(Expression),
    And(Vec<ExpressionTree>),
    Or(Vec<ExpressionTree>),
}

impl Display for ExpressionTree {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpressionTree::Expression(expression) => write!(f, "{}", expression),
            ExpressionTree::And(and_list) => f.write_str(
                &and_list
                    .iter()
                    .map(|expression| format!("({})", expression))
                    .collect::<Vec<String>>()
                    .join(" and "),
            ),
            ExpressionTree::Or(or_list) => f.write_str(
                &or_list
                    .iter()
                    .map(|expression| format!("({})", expression))
                    .collect::<Vec<String>>()
                    .join(" or "),
            ),
        }
    }
}

impl ExpressionTree {
    /// Determines whether the marker expression is true or false in the given environment
    pub fn evaluate(&self, env: &Pep508Environment) -> Result<bool, String> {
        Ok(match self {
            ExpressionTree::Expression(expression) => expression.evaluate(env)?,
            ExpressionTree::And(and_list) => and_list
                .iter()
                .map(|expression| expression.evaluate(env))
                .collect::<Result<Vec<bool>, String>>()?
                .iter()
                .all(|x| *x),
            ExpressionTree::Or(or_list) => or_list
                .iter()
                .map(|expression| expression.evaluate(env))
                .collect::<Result<Vec<bool>, String>>()?
                .iter()
                .any(|x| *x),
        })
    }
}

/// PEP 508 marker parser
pub fn parse_markers(marker: &str) -> Result<ExpressionTree, String> {
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
) -> Result<(ExpressionTree, usize), String> {
    // 😈
    // behold my horror of the parser

    // "and" has precedence over "or", so we make an "or" list that consists of single or a list of "and" nodes
    let mut or_list: Vec<ExpressionTree> = Vec::new();
    let mut and_list = Vec::new();

    let (expression, end_expression) = if chars[start_expression] == '(' {
        parse_expression_list(chars, start_expression + 1)?
    } else {
        let (expression, end_expression) = parse_expression(chars, start_expression)?;
        (ExpressionTree::Expression(expression), end_expression)
    };
    and_list.push(expression);
    let mut start_and_or = position_with_start(chars, end_expression, |c| c.is_whitespace());
    while start_and_or < chars.len() && chars[start_and_or] != ')' {
        let end_and_or = position_with_start(chars, start_and_or, |c| !c.is_whitespace());
        let start_expression = position_with_start(chars, end_and_or, |c| c.is_whitespace());

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
            parse_expression_list(chars, start_expression + 1)?
        } else {
            let (expression, end_expression) = parse_expression(chars, start_expression)?;
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

        start_and_or = position_with_start(chars, end_expression, |c| c.is_whitespace());
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
fn parse_expression(chars: &[char], start_name: usize) -> Result<(Expression, usize), String> {
    let end_name =
        position_with_start(chars, start_name, |c| c.is_ascii_alphanumeric() || c == '_');
    let name = &chars[start_name..end_name];
    let key = Key::from_str(&name.iter().collect::<String>())?;

    let start_comparator = position_with_start(chars, end_name, |c| c.is_whitespace());
    let end_comparator = position_with_start(chars, start_comparator, |c| {
        !c.is_whitespace() && c != '\'' && c != '"'
    });
    let comparator = &chars[start_comparator..end_comparator];
    let comparator = Comparator::from_str(&comparator.iter().collect::<String>())?;

    let start_value_quote = position_with_start(chars, end_comparator, |c| c.is_whitespace());
    // haha yes
    let quote_char = chars[start_value_quote];

    let end_value = position_with_start(chars, start_value_quote + 1, |c| c != quote_char);
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
    use crate::markers::{parse_markers, Pep508Environment};
    use std::path::Path;

    /// The output depends on where we run this, just check it works at all
    /// (which depends on python being in PATH which `cargo test` needs anyway)
    #[test]
    fn get_python() {
        Pep508Environment::from_python(Path::new("python"));
    }

    #[test]
    fn test_marker_evaluation() {
        let env27 = Pep508Environment {
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
        let env37 = Pep508Environment {
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
        let marker1 = parse_markers("python_version == '2.7'").unwrap();
        let marker2 = parse_markers(
            "os_name == \"linux\" or python_version == \"3.7\" and sys_platform == \"win32\"",
        )
        .unwrap();
        let marker3 = parse_markers(
                "python_version == \"2.7\" and (sys_platform == \"win32\" or sys_platform == \"linux\")",
        ).unwrap();
        assert!(marker1.evaluate(&env27).unwrap());
        assert!(!marker1.evaluate(&env37).unwrap());
        assert!(marker2.evaluate(&env27).unwrap());
        assert!(marker2.evaluate(&env37).unwrap());
        assert!(marker3.evaluate(&env27).unwrap());
        assert!(!marker3.evaluate(&env37).unwrap());
    }

    /// Copied from <https://github.com/pypa/packaging/blob/85ff971a250dc01db188ef9775499c15553a8c95/tests/test_markers.py#L175-L221>
    #[test]
    fn test_marker_equivalence() {
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
            assert_eq!(parse_markers(a).unwrap(), parse_markers(b).unwrap());
        }
    }
}
