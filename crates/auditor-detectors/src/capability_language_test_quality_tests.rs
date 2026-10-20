//! Test-quality capability boundaries for each Tier-1 framework. Positives
//! hold only tests the detector must report; negatives hold the nearest
//! forms it must not (delegated, skipped, expected-failure, and smoke tests;
//! identity tests, impure operands, and mocks that feed the code under test).

use std::collections::BTreeMap;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

pub(crate) fn test_quality_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let files: Vec<(&str, &str)> = match (detector, language, positive) {
        ("test_without_assertion", "rust", true) => vec![
            ("tests/parser.rs", RUST_HOLLOW),
            ("src/lib.rs", RUST_INLINE_HOLLOW),
        ],
        ("test_without_assertion", "rust", false) => vec![("tests/parser.rs", RUST_ASSERTING)],
        ("tautological_assertion", "rust", true) => vec![("tests/parser.rs", RUST_TAUTOLOGIES)],
        ("tautological_assertion", "rust", false) => vec![("tests/parser.rs", RUST_SOUND)],
        ("test_without_assertion", "python", true) => vec![
            ("tests/test_parser.py", PYTHON_HOLLOW),
            ("src/service.py", PYTHON_PRODUCTION),
        ],
        ("test_without_assertion", "python", false) => vec![
            ("tests/test_parser.py", PYTHON_ASSERTING),
            ("tests/data/cases/fmtonoff.py", PYTHON_HOLLOW),
            ("tests/support.py", PYTHON_SUPPORT),
            ("src/service.py", PYTHON_PRODUCTION),
        ],
        ("tautological_assertion", "python", true) => {
            vec![("tests/test_parser.py", PYTHON_TAUTOLOGIES)]
        }
        ("tautological_assertion", "python", false) => vec![("tests/test_parser.py", PYTHON_SOUND)],
        ("test_without_assertion", "javascript" | "typescript", true) => {
            vec![(js_path(language), JS_HOLLOW)]
        }
        ("test_without_assertion", "javascript", false) => {
            vec![(js_path(language), JS_ASSERTING)]
        }
        ("test_without_assertion", "typescript", false) => vec![
            (js_path(language), JS_ASSERTING),
            ("src/client.test.ts", TS_TYPE_TESTS),
            ("src/types.test.ts", JS_HOLLOW),
        ],
        ("tautological_assertion", "javascript" | "typescript", true) => {
            vec![(js_path(language), JS_TAUTOLOGIES)]
        }
        ("tautological_assertion", "javascript" | "typescript", false) => {
            vec![(js_path(language), JS_SOUND)]
        }
        ("test_without_assertion", "go", true) => vec![("parser_test.go", GO_HOLLOW)],
        ("test_without_assertion", "go", false) => vec![("parser_test.go", GO_ASSERTING)],
        ("tautological_assertion", "go", true) => vec![("parser_test.go", GO_TAUTOLOGIES)],
        ("tautological_assertion", "go", false) => vec![("parser_test.go", GO_SOUND)],
        ("test_without_assertion", "java", true) => vec![(JAVA_PATH, JAVA_HOLLOW)],
        ("test_without_assertion", "java", false) => vec![(JAVA_PATH, JAVA_ASSERTING)],
        ("tautological_assertion", "java", true) => vec![(JAVA_PATH, JAVA_TAUTOLOGIES)],
        ("tautological_assertion", "java", false) => vec![(JAVA_PATH, JAVA_SOUND)],
        _ => return None,
    };
    Some(
        files
            .into_iter()
            .map(|(path, source)| (path.to_string(), source.to_string()))
            .collect(),
    )
}

fn js_path(language: &str) -> &'static str {
    if language == "typescript" {
        "src/parser.test.ts"
    } else {
        "src/parser.test.js"
    }
}

const JAVA_PATH: &str = "src/test/java/ParserTest.java";

const RUST_HOLLOW: &str = r#"use parser::parse;

#[test]
fn hollow_parse() {
    let _ = parse("x");
}

#[tokio::test]
async fn hollow_async() {
    parse("x");
}
"#;

const RUST_INLINE_HOLLOW: &str = r#"pub fn parse(input: &str) -> usize {
    input.len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hollow_inline() {
        super::parse("x");
    }
}
"#;

const RUST_ASSERTING: &str = r#"use parser::parse;

fn assert_round_trip(value: usize) {
    assert_eq!(decode(encode(value)), value);
}

fn round_trip_twice(value: usize) {
    assert_round_trip(value);
}

#[test]
fn asserts_parse() {
    assert_eq!(parse("x"), 1);
}

#[test]
fn unwraps_parse() {
    parse("x").unwrap();
}

#[test]
fn delegates_transitively() {
    round_trip_twice(3);
}

#[test]
fn propagates() -> Result<(), Error> {
    parse("x")?;
    Ok(())
}

#[test]
#[should_panic]
fn panics_on_empty() {
    parse("");
}

#[test]
#[ignore]
fn pending() {
    parse("x");
}

#[test]
fn parse_does_not_panic() {
    parse("x");
}

#[test]
fn traits() {
    assert_send::<Router>();
}

#[test]
fn custom_macro() {
    test_parse_error!(vec![("a", "1")], u32, ErrorKind::ParseError);
}

#[test]
fn routes_type_check() {
    let _: MethodRouter<()> = get(ok).head(ok);
}

#[test]
fn prints_unwrapped() {
    println!("{:?}", hostname().unwrap());
}

fn not_a_test() {
    parse("x");
}
"#;

const RUST_TAUTOLOGIES: &str = r#"#[test]
fn reads_value_twice() {
    let value = parse("x");
    assert_eq!(3, 3);
    assert!(true);
}

#[test]
fn mock_returns_stub() {
    let mut mock = MockStore::new();
    mock.expect_get().return_const(5);
    assert_eq!(mock.get(1), 5);
}
"#;

const RUST_SOUND: &str = r#"#[test]
fn compares_with_expected() {
    let value = parse("x");
    assert_eq!(value, 1);
    assert!(value == 1);
}

#[test]
fn eq_is_reflexive() {
    let value = parse("x");
    assert_eq!(value, value);
}

#[test]
fn calls_twice() {
    assert_eq!(next_id(), next_id());
}

#[test]
fn lexes_token() {
    let token = lex("x");
    assert_eq!(token, token);
}

#[test]
fn mock_feeds_code() {
    let mut mock = MockStore::new();
    mock.expect_get().return_const(5);
    assert_eq!(total(&mock), 5);
}
"#;

const PYTHON_HOLLOW: &str = r#"from parser import parse


def test_hollow():
    parse("x")


class TestParser(unittest.TestCase):
    def test_hollow_method(self):
        parse("x")
"#;

const PYTHON_PRODUCTION: &str = r#"def test_connection():
    connect()
"#;

const PYTHON_SUPPORT: &str = r#"def roundtrip(value):
    assert decode(encode(value)) == value
"#;

const PYTHON_ASSERTING: &str = r#"import pytest
from support import roundtrip
from parser import parse


def assert_parsed(value):
    assert value is not None


def test_asserts():
    assert parse("x") == 1


def test_helper():
    assert_parsed(parse("x"))


def test_cross_file_helper():
    roundtrip(3)


def test_raises():
    with pytest.raises(ValueError):
        parse("")


def test_parse_does_not_raise():
    parse("x")


@pytest.mark.skip(reason="pending")
def test_pending():
    parse("x")


def test_parse_speed(benchmark):
    benchmark(parse, "x")


class TestParser(unittest.TestCase):
    def test_inherited_helper(self):
        self.run_case("x")


class Helper:
    def test_not_collected(self):
        parse("x")


def helper_without_test_prefix():
    parse("x")
"#;

const PYTHON_TAUTOLOGIES: &str = r#"def test_reads_twice():
    value = parse("x")
    assert value is value
    assert True
    assert (value, "message")


def test_mock_returns_stub():
    store = Mock()
    store.get.return_value = 5
    assert store.get(1) == 5


class TestParser(unittest.TestCase):
    def test_unittest_forms(self):
        self.assertIs(parse, parse)
        self.assertTrue(True)
"#;

const PYTHON_SOUND: &str = r#"def test_expected():
    assert parse("x") == 1


def test_equality_is_reflexive():
    value = parse("x")
    assert value == value


def test_lexes():
    token = lex("x")
    assert token == token


class TestLexer(unittest.TestCase):
    def test_lexes(self):
        token = lex("x")
        self.assertEqual(token, token)


def test_calls_twice():
    assert next_id() == next_id()


def test_not_a_number():
    value = float("nan")
    assert value != value


def test_unreachable():
    assert False


def test_mock_feeds_code():
    store = Mock()
    store.get.return_value = 5
    assert total(store) == 5
"#;

const JS_HOLLOW: &str = r#"const { parse } = require('./parser');

describe('parser', () => {
  it('hollow', () => {
    parse('x');
  });

  test('hollow function', function () {
    parse('y');
  });
});
"#;

const JS_ASSERTING: &str = r#"const { parse } = require('./parser');

function expectParsed(value) {
  expect(value).toBeDefined();
}

const roundTrip = (value) => {
  expect(decode(encode(value))).toEqual(value);
};

describe('parser', () => {
  it('asserts', () => {
    expect(parse('x')).toBe(1);
  });

  it('delegates', () => {
    expectParsed(parse('x'));
  });

  it('round trips', () => {
    roundTrip(3);
  });

  it.skip('pending', () => {
    parse('x');
  });

  it.todo('later');

  it('renders without crashing', () => {
    parse('x');
  });

  it('calls back', (done) => {
    parse('x', done);
  });

  test('ava style', (t) => {
    t.is(parse('x'), 1);
  });

  it('supertest', async () => {
    await request(app).get('/').expect(200);
  });

  it('routes', function (done) {
    router.handle({ url: '/foo' }, { end: done });
  });

  test('go to /', async ({ page }) => {
    await page.goto('/');
    await page.waitForSelector('text=Starter');
  });

  it('should not exit the process unexpectedly', async () => {
    await execFile(process.execPath, ['crash.js']);
  });

  it('constructor accepts defaults', () => {
    new Parser();
  });
});
"#;

const TS_TYPE_TESTS: &str = r#"describe('client', () => {
  it('response shape', () => {
    const req = client.index.$get;
    type Actual = ResponseOf<typeof req>;
    type verify = Expect<Equal<{ data: string }, Actual>>;
  });

  it('rejects a wrong return', () => {
    // @ts-expect-error
    produce(base, () => ({ a: true }));
  });

  it('is a document superset', () => {
    const published: Document = null as unknown as OpenApiDocument;
  });

  it('accepts JSON', () => {
    isType.equal<true, JsonValue, Draft<JsonValue>>();
  });

  it('good placeholder does not typeerror', () => {
    client.post.list.useQuery(undefined, { placeholderData: () => [] });
  });
});
"#;

const JS_TAUTOLOGIES: &str = r#"describe('parser', () => {
  it('reads twice', () => {
    const value = parse('x');
    expect(value).toBe(value);
    expect(true).toBeTruthy();
    assert.equal(value, value);
  });

  it('mock returns stub', () => {
    const fetch = jest.fn().mockReturnValue(5);
    expect(fetch()).toBe(5);
    store.get.mockReturnValue(7);
    expect(store.get(1)).toBe(7);
  });
});
"#;

const JS_SOUND: &str = r#"describe('parser', () => {
  it('expected', () => {
    expect(parse('x')).toBe(1);
  });

  it('negated', () => {
    const value = parse('x');
    expect(value).not.toBe(value);
  });

  it('fresh literals', () => {
    expect([]).toEqual([]);
  });

  it('calls twice', () => {
    expect(nextId()).toBe(nextId());
  });

  it('mock feeds code', () => {
    const fetch = jest.fn().mockReturnValue(5);
    expect(total(fetch)).toBe(5);
  });

  it('spy wraps a prototype method', () => {
    const spy = jest.spyOn(schema, 'safeParse').mockReturnValue(7);
    expect(schema.safeParse({})).toBe(7);
  });
});
"#;

const GO_HOLLOW: &str = r#"package parser

import "testing"

func TestHollow(t *testing.T) {
	Parse("x")
}

func TestLogsOnly(t *testing.T) {
	t.Log(Parse("x"))
}
"#;

const GO_ASSERTING: &str = r#"package parser

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func checkParsed(t *testing.T, value int) {
	t.Helper()
	if value != 1 {
		t.Fatalf("got %d", value)
	}
}

func TestReports(t *testing.T) {
	if Parse("x") != 1 {
		t.Errorf("unexpected")
	}
}

func TestHelper(t *testing.T) {
	checkParsed(t, Parse("x"))
}

func TestSubtests(t *testing.T) {
	t.Run("case", func(t *testing.T) {
		assert.Equal(t, 1, Parse("x"))
	})
}

func TestSkipped(t *testing.T) {
	t.Skip("pending")
	Parse("x")
}

func TestParseDoesNotPanic(t *testing.T) {
	Parse("x")
}

func TestCalledAs(t *testing.T) {
	tests := map[string]calledAsTestcase{"find": {}}
	for name, tc := range tests {
		t.Run(name, tc.test)
	}
}

func (tc *calledAsTestcase) test(t *testing.T) {
	if tc.want != Find(tc.args) {
		t.Errorf("unexpected")
	}
}

func TestCompletionSignature(t *testing.T) {
	var userComp func(args []string) []string
	var _ CompletionFunc = userComp
}

func Testlowercase(t *testing.T) {
	Parse("x")
}

func BenchmarkParse(b *testing.B) {
	Parse("x")
}
"#;

const GO_TAUTOLOGIES: &str = r#"package parser

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestReadsTwice(t *testing.T) {
	value := Parse("x")
	assert.Equal(t, value, value)
	assert.True(t, true)
}

func TestMockReturnsStub(t *testing.T) {
	store := new(MockStore)
	store.On("Get", 1).Return(5)
	assert.Equal(t, 5, store.Get(1))
}
"#;

const GO_SOUND: &str = r#"package parser

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestExpected(t *testing.T) {
	assert.Equal(t, 1, Parse("x"))
}

func TestCallsTwice(t *testing.T) {
	assert.Equal(t, NextID(), NextID())
}

func TestMockFeedsCode(t *testing.T) {
	store := new(MockStore)
	store.On("Get", 1).Return(5)
	assert.Equal(t, 5, Total(store))
}
"#;

const JAVA_HOLLOW: &str = r#"import org.junit.jupiter.api.Test;

class ParserTest {
    @Test
    void hollow() {
        parser.parse("x");
    }
}

class LegacyTest extends TestCase {
    public void testHollowLegacy() {
        parser.parse("x");
    }
}
"#;

const JAVA_ASSERTING: &str = r#"import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

class ParserTest extends BaseTest {
    private void assertParsed(int value) {
        assertNotNull(value);
    }

    @Test
    void asserts() {
        assertEquals(1, parser.parse("x"));
    }

    @Test
    void delegates() {
        assertParsed(parser.parse("x"));
    }

    @Test
    void inherited() {
        runCase("x");
    }

    @Test(expected = IllegalArgumentException.class)
    public void rejectsEmpty() {
        parser.parse("");
    }

    @Test
    @Disabled
    void pending() {
        parser.parse("x");
    }

    @Test
    void contextLoads() {
    }

    @Test
    void verifiesMock() {
        parser.parse("x");
        verify(store).save(any());
    }

    @Test
    void mockMvc() throws Exception {
        mockMvc.perform(get("/")).andExpect(status().isOk());
    }

    @Test
    void testConstructor() {
        new Parser();
    }

    void helperNotTest() {
        parser.parse("x");
    }
}

class LegacyTest extends TestCase {
    public void helperNotTest() {
        parser.parse("x");
    }
}
"#;

const JAVA_TAUTOLOGIES: &str = r#"class ParserTest {
    @Test
    void readsTwice() {
        int value = parser.parse("x");
        assertSame(value, value);
        assertTrue(true);
        assertThat(value).isSameAs(value);
        assertEquals(1, 1);
    }

    @Test
    void mockReturnsStub() {
        when(store.get(1)).thenReturn(5);
        assertEquals(5, store.get(1));
    }
}
"#;

const JAVA_SOUND: &str = r#"class ParserTest {
    @Test
    void expected() {
        assertEquals(1, parser.parse("x"));
        assertTrue("message", parser.ready());
    }

    @Test
    void equalsIsReflexive() {
        Money money = new Money(3);
        assertEquals(money, money);
    }

    @Test
    void lexes() {
        Token token = lexer.next();
        assertEquals(token, token);
        assertThat(token).isEqualTo(token);
        assertThat(token, is(token));
    }

    @Test
    void callsTwice() {
        assertEquals(ids.next(), ids.next());
    }

    @Test
    void mockFeedsCode() {
        when(store.get(1)).thenReturn(5);
        assertEquals(5, service.total(store));
    }
}
"#;

/// A detector, a language, and the tests (with finding counts) its positive
/// boundary reports.
type Boundary = (&'static str, &'static str, &'static [(&'static str, usize)]);

/// The tests (with their finding counts) each positive must report, and
/// the matching negative must report none.
#[test]
fn test_quality_boundaries_report_exactly_the_hollow_and_tautological_tests() {
    let expected: &[Boundary] = &[
        (
            "test_without_assertion",
            "rust",
            &[
                ("hollow_parse", 1),
                ("hollow_async", 1),
                ("hollow_inline", 1),
            ],
        ),
        (
            "test_without_assertion",
            "python",
            &[("test_hollow", 1), ("test_hollow_method", 1)],
        ),
        (
            "test_without_assertion",
            "javascript",
            &[("hollow", 1), ("hollow function", 1)],
        ),
        (
            "test_without_assertion",
            "typescript",
            &[("hollow", 1), ("hollow function", 1)],
        ),
        (
            "test_without_assertion",
            "go",
            &[("TestHollow", 1), ("TestLogsOnly", 1)],
        ),
        (
            "test_without_assertion",
            "java",
            &[("hollow", 1), ("testHollowLegacy", 1)],
        ),
        (
            "tautological_assertion",
            "rust",
            &[("reads_value_twice", 2), ("mock_returns_stub", 1)],
        ),
        (
            "tautological_assertion",
            "python",
            &[
                ("test_reads_twice", 3),
                ("test_mock_returns_stub", 1),
                ("test_unittest_forms", 2),
            ],
        ),
        (
            "tautological_assertion",
            "javascript",
            &[("reads twice", 3), ("mock returns stub", 2)],
        ),
        (
            "tautological_assertion",
            "typescript",
            &[("reads twice", 3), ("mock returns stub", 2)],
        ),
        (
            "tautological_assertion",
            "go",
            &[("TestReadsTwice", 2), ("TestMockReturnsStub", 1)],
        ),
        (
            "tautological_assertion",
            "java",
            &[("readsTwice", 4), ("mockReturnsStub", 1)],
        ),
    ];
    let mut failures = Vec::new();
    for (detector, language, tests) in expected {
        for positive in [true, false] {
            let files = test_quality_capability_files(detector, language, positive).unwrap();
            let mut case = CapabilityCase::new(detector, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let mut reported = BTreeMap::<String, usize>::new();
            for finding in analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == *detector)
            {
                let test = finding
                    .message
                    .split('`')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                *reported.entry(test).or_default() += 1;
            }
            let wanted = if positive {
                tests
                    .iter()
                    .map(|(name, count)| (name.to_string(), *count))
                    .collect()
            } else {
                BTreeMap::new()
            };
            if reported != wanted {
                failures.push(format!(
                    "{detector} {language} {}: reported {reported:?}, expected {wanted:?}",
                    if positive { "positive" } else { "negative" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
