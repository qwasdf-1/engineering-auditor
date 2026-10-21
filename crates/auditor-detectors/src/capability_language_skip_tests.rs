//! `skipped_test_without_reason` capability boundaries for each Tier-1
//! framework. Positives hold skips with no reason (beside a doc comment, a
//! lint directive, or a URL in the title, none of which is one); negatives
//! hold every form of a stated reason: an argument, a comment, a condition.

use std::collections::BTreeMap;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "skipped_test_without_reason";

pub(crate) fn skip_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector != DETECTOR {
        return None;
    }
    let (path, source) = match (language, positive) {
        ("rust", true) => ("tests/parser.rs", RUST_UNEXPLAINED),
        ("rust", false) => ("tests/parser.rs", RUST_EXPLAINED),
        ("python", true) => ("tests/test_parser.py", PYTHON_UNEXPLAINED),
        ("python", false) => ("tests/test_parser.py", PYTHON_EXPLAINED),
        ("javascript", true) => ("src/parser.test.js", JS_UNEXPLAINED),
        ("javascript", false) => ("src/parser.test.js", JS_EXPLAINED),
        ("typescript", true) => ("src/parser.test.ts", JS_UNEXPLAINED),
        ("typescript", false) => ("src/parser.test.ts", JS_EXPLAINED),
        ("go", true) => ("parser_test.go", GO_UNEXPLAINED),
        ("go", false) => ("parser_test.go", GO_EXPLAINED),
        ("java", true) => (JAVA_PATH, JAVA_UNEXPLAINED),
        ("java", false) => (JAVA_PATH, JAVA_EXPLAINED),
        _ => return None,
    };
    Some(vec![(path.to_string(), source.to_string())])
}

const JAVA_PATH: &str = "src/test/java/ParserTest.java";

const RUST_UNEXPLAINED: &str = r#"use parser::parse;

#[test]
#[ignore]
fn slow_parse() {
    assert_eq!(parse("a"), 1);
}

/// Checks the slow path.
#[test]
#[ignore]
fn documented_slow_path() {
    assert_eq!(parse("b"), 2);
}
"#;

const RUST_EXPLAINED: &str = r#"use parser::parse;

#[test]
#[ignore = "needs a network connection"]
fn fetches_remote() {
    assert!(parser::fetch().is_ok());
}

// Flaky on CI runners: the timer resolution is too coarse.
#[test]
#[ignore]
fn flaky_timing() {
    assert!(parser::timing() < 10);
}

#[test]
#[ignore] // takes ten minutes
fn exhaustive() {
    assert!(parser::exhaustive());
}

#[test]
#[cfg_attr(windows, ignore)]
fn unix_paths() {
    assert_eq!(parse("/"), 0);
}

#[test]
#[ignore]
fn writes_empty_slice() {
    // TODO: needs vectored writes back.
    assert!(parser::write_empty());
}

#[test]
fn parses() {
    assert_eq!(parse("a"), 1);
}
"#;

const PYTHON_UNEXPLAINED: &str = r#"import pytest


@pytest.mark.skip
def test_slow():
    assert parse("a") == 1


@pytest.mark.skip()
def test_empty_call():
    assert parse("b") == 2


@pytest.mark.skip(reason="")
def test_empty_reason():
    assert parse("c") == 3


@pytest.mark.skip
def test_documented():
    """Parses a document with a byte-order mark."""
    assert parse("\ufeff") == 0


@pytest.mark.skip
class TestLegacy:
    def test_old(self):
        assert parse("d") == 4
"#;

const PYTHON_EXPLAINED: &str = r#"import sys
import unittest

import pytest


@pytest.mark.skip(reason="the service sandbox is down")
def test_service():
    assert call() == 1


@pytest.mark.skip("needs a GPU")
def test_gpu():
    assert gpu() == 1


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX paths")
def test_paths():
    assert paths() == 1


# Disabled until the parser handles tabs again.
@pytest.mark.skip
def test_tabs():
    assert parse("\t") == 0


@pytest.mark.skip  # flaky under load
def test_load():
    assert load() == 1


@pytest.mark.skip
def test_pypy():
    """Fails on PyPy: the C extension is not built there."""
    assert native() == 1


@pytest.mark.xfail
def test_known_bug():
    assert bug() == 1


class ParserTests(unittest.TestCase):
    @unittest.skip("covered by the integration suite")
    def test_integration(self):
        self.assertEqual(run(), 1)


@pytest.mark.skip(reason="replaced by the streaming parser")
class TestLegacy:
    def test_old(self):
        assert parse("d") == 4
"#;

const JS_UNEXPLAINED: &str = r#"describe('parser', () => {
  it.skip('parses tabs', () => {
    expect(parse('\t')).toBe(0);
  });
  xit('parses spaces', () => {
    expect(parse(' ')).toBe(1);
  });
  // eslint-disable-next-line jest/no-disabled-tests
  test.skip('parses newlines', () => {
    expect(parse('\n')).toBe(2);
  });
  it.skip('loads http://example.com/a', () => {
    expect(load('http://example.com/a')).toBeTruthy();
  });
});

describe.skip('legacy parser', () => {
  it('parses', () => {
    expect(parse('a')).toBe(1);
  });
});
"#;

const JS_EXPLAINED: &str = r#"describe('parser', () => {
  // Flaky on CI: the fixture server times out.
  it.skip('fetches fixtures', () => {
    expect(fetchFixtures()).toBeTruthy();
  });
  it.skip('parses unicode', () => { // waiting on the ICU upgrade
    expect(parse('é')).toBe(1);
  });
  /**
   * NOTE: skipped because the digest endpoint no longer resolves.
   */
  test.skip('sends digest auth', () => {
    expect(sendDigest()).toBeTruthy();
  });
  it.todo('parses emoji');
  it('parses ascii', () => {
    expect(parse('a')).toBe(1);
  });
});

/* The legacy parser is removed in the next major version. */
describe.skip('legacy parser', () => {
  it('parses', () => {
    expect(parse('a')).toBe(1);
  });
});
"#;

const GO_UNEXPLAINED: &str = r#"package parser

import "testing"

func TestTabs(t *testing.T) {
	t.Skip()
	if Parse("\t") != 0 {
		t.Fatal("tab")
	}
}

func TestSpaces(t *testing.T) {
	t.SkipNow()
	if Parse(" ") != 1 {
		t.Fatal("space")
	}
}
"#;

const GO_EXPLAINED: &str = r#"package parser

import (
	"runtime"
	"testing"
)

func TestService(t *testing.T) {
	t.Skip("the service sandbox is down")
	if Call() != 1 {
		t.Fatal("call")
	}
}

func TestPaths(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.SkipNow()
	}
	if Parse("/") != 0 {
		t.Fatal("path")
	}
}

func TestFlaky(t *testing.T) {
	// Flaky under the race detector; see the tracking issue.
	t.Skip()
	if Parse("a") != 1 {
		t.Fatal("a")
	}
}

func TestRuns(t *testing.T) {
	if Parse("a") != 1 {
		t.Fatal("a")
	}
}
"#;

const JAVA_UNEXPLAINED: &str = r#"import org.junit.jupiter.api.Disabled;
import org.junit.jupiter.api.Test;

class ParserTest {
    @Test
    @Disabled
    void parsesTabs() {
        assertEquals(0, Parser.parse("\t"));
    }

    /** Checks spaces. */
    @Test
    @Disabled
    void parsesSpaces() {
        assertEquals(1, Parser.parse(" "));
    }

    @Test(enabled = false)
    void parsesNewlines() {
        assertEquals(2, Parser.parse("\n"));
    }
}

@Disabled
class LegacyParserTest {
    @Test
    void parses() {
        assertEquals(1, LegacyParser.parse("a"));
    }
}

/**
 * Tests concurrent access to the registry.
 *
 * @see <a href="https://issues.example.org/browse/LANG-762">LANG-762</a>
 */
class ConcurrencyTest {
    @Test
    @Disabled
    void concurrentAccess() {
        assertEquals(1, Registry.access());
    }
}
"#;

const JAVA_EXPLAINED: &str = r#"import org.junit.Ignore;
import org.junit.jupiter.api.Disabled;
import org.junit.jupiter.api.Test;

class ParserTest {
    @Test
    @Disabled("waiting on the tokenizer rewrite")
    void parsesTabs() {
        assertEquals(0, Parser.parse("\t"));
    }

    // Flaky on CI: depends on the system locale.
    @Test
    @Disabled
    void parsesLocale() {
        assertEquals(1, Parser.parse("é"));
    }

    @Test
    @Ignore("JUnit 4 form, with a reason")
    public void parsesLegacy() {
        assertEquals(2, Parser.parse("b"));
    }

    /**
     * This test fails because the tokenizer drops the sign.
     */
    @Test
    @Disabled
    void parsesNegative() {
        assertEquals(-1, Parser.parse("-1"));
    }

    @Test
    @Disabled
    void parsesKorean() {
        // LANG-1655
        assertEquals(3, Parser.parse("한"));
    }

    @Test
    void parsesAscii() {
        assertEquals(1, Parser.parse("a"));
    }
}

/** Measures performance. All tests here are disabled; run them by hand. */
class PerformanceTest {
    @Test
    @Ignore
    public void serializesLargeCollections() {
        assertEquals(0, Perf.run());
    }
}

@Disabled("superseded by ParserTest")
class LegacyParserTest {
    @Test
    void parses() {
        assertEquals(1, LegacyParser.parse("a"));
    }
}
"#;

/// Each positive reports exactly these tests and groups, and its negative
/// none.
#[test]
fn skip_boundaries_report_exactly_the_unexplained_skips() {
    let expected: &[(&str, &[&str])] = &[
        ("rust", &["slow_parse", "documented_slow_path"]),
        (
            "python",
            &[
                "test_slow",
                "test_empty_call",
                "test_empty_reason",
                "test_documented",
                "TestLegacy",
            ],
        ),
        (
            "javascript",
            &[
                "parses tabs",
                "parses spaces",
                "parses newlines",
                "loads http://example.com/a",
                "legacy parser",
            ],
        ),
        (
            "typescript",
            &[
                "parses tabs",
                "parses spaces",
                "parses newlines",
                "loads http://example.com/a",
                "legacy parser",
            ],
        ),
        ("go", &["TestTabs", "TestSpaces"]),
        (
            "java",
            &[
                "parsesTabs",
                "parsesSpaces",
                "parsesNewlines",
                "LegacyParserTest",
                "concurrentAccess",
            ],
        ),
    ];
    let mut failures = Vec::new();
    for (language, names) in expected {
        for positive in [true, false] {
            let files = skip_capability_files(DETECTOR, language, positive).unwrap();
            let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let mut reported = BTreeMap::<String, usize>::new();
            for finding in analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == DETECTOR)
            {
                let name = finding.message.split('`').nth(1).unwrap_or_default();
                *reported.entry(name.to_string()).or_default() += 1;
            }
            let wanted = if positive {
                names.iter().map(|name| (name.to_string(), 1)).collect()
            } else {
                BTreeMap::new()
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
