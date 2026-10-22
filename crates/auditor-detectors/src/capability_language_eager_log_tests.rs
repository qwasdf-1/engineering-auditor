//! `eager_log_formatting` capability boundaries for each Tier-1 language
//! with a level-checking logger API. Positives format a debug or trace
//! message before the call; negatives hold the nearest lazy forms: values
//! passed as arguments, a constant message, an enabled-level guard, another
//! level, a receiver that is not a logger, and eager messages in test code.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "eager_log_formatting";

pub(crate) fn eager_log_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector != DETECTOR {
        return None;
    }
    let files: Vec<(&str, &str)> = match (language, positive) {
        ("python", true) => vec![("src/service.py", PYTHON_EAGER)],
        ("python", false) => vec![
            ("src/service.py", PYTHON_LAZY),
            ("tests/test_service.py", PYTHON_EAGER),
        ],
        ("javascript", true) => vec![("src/service.js", JS_EAGER)],
        ("javascript", false) => vec![
            ("src/service.js", JS_LAZY),
            ("src/service.test.js", JS_EAGER),
        ],
        ("typescript", true) => vec![("src/service.ts", JS_EAGER)],
        ("typescript", false) => vec![
            ("src/service.ts", JS_LAZY),
            ("src/service.test.ts", JS_EAGER),
        ],
        ("go", true) => vec![("service.go", GO_EAGER)],
        ("go", false) => vec![("service.go", GO_LAZY), ("service_test.go", GO_EAGER)],
        ("java", true) => vec![("src/main/java/Service.java", JAVA_EAGER)],
        ("java", false) => vec![
            ("src/main/java/Service.java", JAVA_LAZY),
            ("src/test/java/ServiceTest.java", JAVA_EAGER),
        ],
        _ => return None,
    };
    Some(
        files
            .into_iter()
            .map(|(path, source)| (path.to_string(), source.to_string()))
            .collect(),
    )
}

const PYTHON_EAGER: &str = r#"import logging

logger = logging.getLogger(__name__)


def handle(user, items):
    logger.debug(f"user {user} has {len(items)} items")
    logger.debug("user %s" % user)
    logging.debug("user {}".format(user))
    self_log = logger
    self_log.debug("user " + user.name)
"#;

const PYTHON_LAZY: &str = r#"import logging

logger = logging.getLogger(__name__)


def handle(user, catalog):
    logger.debug("user %s", user)
    logger.debug(f"starting")
    logger.debug("a" + "b")
    logger.info(f"user {user}")
    if logger.isEnabledFor(logging.DEBUG):
        logger.debug(f"user {user}")
    catalog.debug(f"user {user}")
"#;

const JS_EAGER: &str = r#"function handle(user) {
  logger.debug(`user ${user.id}`);
  this.log.trace("user " + user.id);
}
"#;

const JS_LAZY: &str = r#"function handle(user) {
  logger.debug("user %s", user.id);
  logger.debug(`starting`);
  logger.info(`user ${user.id}`);
  console.debug(`user ${user.id}`);
}
"#;

const GO_EAGER: &str = r#"package service

import (
	"fmt"
	"log/slog"
)

func Handle(user string) {
	logger.Debug(fmt.Sprintf("user %s", user))
	slog.Debug("user " + user)
}
"#;

const GO_LAZY: &str = r#"package service

import (
	"fmt"
	"log/slog"
)

func Handle(user string) {
	logger.Debugf("user %s", user)
	slog.Debug("user", "name", user)
	logger.Info(fmt.Sprintf("user %s", user))
}
"#;

const JAVA_EAGER: &str = r#"class Service {
    void handle(String user) {
        LOG.debug("user " + user);
        log.trace(String.format("user %s", user));
    }
}
"#;

const JAVA_LAZY: &str = r#"class Service {
    void handle(String user) {
        log.debug("user {}", user);
        log.debug("a" + "b");
        log.info("user " + user);
        if (log.isDebugEnabled()) {
            log.debug("user " + user);
        }
    }
}
"#;

#[test]
fn eager_log_boundaries_report_exactly_the_formatted_messages() {
    let js = ["`user ${user.id}`", "\"user \" + user.id"];
    let expected: &[(&str, &[&str])] = &[
        (
            "python",
            &[
                "f\"user {user} has {len(items)} items\"",
                "\"user %s\" % user",
                "\"user {}\".format(user)",
                "\"user \" + user.name",
            ],
        ),
        ("javascript", &js),
        ("typescript", &js),
        (
            "go",
            &["fmt.Sprintf(\"user %s\", user)", "\"user \" + user"],
        ),
        (
            "java",
            &["\"user \" + user", "String.format(\"user %s\", user)"],
        ),
    ];
    let mut failures = Vec::new();
    for (language, messages) in expected {
        for positive in [true, false] {
            let files = eager_log_capability_files(DETECTOR, language, positive).unwrap();
            let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let reported = analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == DETECTOR)
                .filter_map(|finding| finding.identity.get("message").cloned())
                .collect::<BTreeSet<_>>();
            let wanted = if positive {
                messages.iter().map(|message| message.to_string()).collect()
            } else {
                BTreeSet::new()
            };
            if reported != wanted {
                failures.push(format!(
                    "{language} {}: reported {reported:?}, expected {wanted:?}",
                    if positive { "positive" } else { "negative" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
