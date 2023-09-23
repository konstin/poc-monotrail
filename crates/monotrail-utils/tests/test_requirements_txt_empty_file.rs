use logtest::Logger;
use monotrail_utils::RequirementsTxt;
use std::path::Path;
use tracing::log::Level;

// NOTE: Prevent race conditions by running isolated from other tests
//  See https://github.com/yoshuawuyts/logtest/blob/a6da0057fb52ec702e89eadf4689e3a56a97099b/src/lib.rs#L12-L16
#[test]
fn test_empty_requirements_file() {
    let working_dir = Path::new("../../test-data").join("requirements-txt");
    let path = working_dir.join("empty.txt");

    let logger = Logger::start();
    RequirementsTxt::parse(path, &working_dir).unwrap();
    let warnings: Vec<_> = logger
        .into_iter()
        .filter(|message| message.level() >= Level::Warn)
        .collect();
    assert_eq!(warnings.len(), 1, "{:?}", warnings);
    assert!(warnings[0]
        .args()
        .ends_with("does not contain any dependencies"));
}
