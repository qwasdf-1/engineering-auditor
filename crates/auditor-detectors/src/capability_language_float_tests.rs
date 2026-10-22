//! `float_equality_comparison` capability boundaries for each Tier-1 and
//! Tier-2 language. Positives compare computed floats (an arithmetic expression, a
//! local computed by arithmetic, a `+=` accumulation) with inexact
//! literals; negatives hold the nearest forms that compare equal by
//! construction or are exact: a parameter, a parsed value, an exact literal,
//! an integer, and computed comparisons in test code.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "float_equality_comparison";

pub(crate) fn float_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector != DETECTOR {
        return None;
    }
    let files: Vec<(&str, &str)> = match (language, positive) {
        ("python", true) => vec![("src/ratio.py", PYTHON_COMPUTED)],
        ("python", false) => vec![
            ("src/ratio.py", PYTHON_SAFE),
            ("tests/test_ratio.py", PYTHON_COMPUTED),
        ],
        ("javascript", true) => vec![("src/ratio.js", JS_COMPUTED)],
        ("javascript", false) => vec![
            ("src/ratio.js", JS_SAFE),
            ("src/ratio.test.js", JS_COMPUTED),
        ],
        ("typescript", true) => vec![("src/ratio.ts", JS_COMPUTED)],
        ("typescript", false) => vec![
            ("src/ratio.ts", JS_SAFE),
            ("src/ratio.test.ts", JS_COMPUTED),
        ],
        ("rust", true) => vec![("src/ratio.rs", RUST_COMPUTED)],
        ("rust", false) => vec![("src/ratio.rs", RUST_SAFE)],
        ("go", true) => vec![("ratio.go", GO_COMPUTED)],
        ("go", false) => vec![("ratio.go", GO_SAFE), ("ratio_test.go", GO_COMPUTED)],
        ("java", true) => vec![("src/main/java/Ratio.java", JAVA_COMPUTED)],
        ("java", false) => vec![
            ("src/main/java/Ratio.java", JAVA_SAFE),
            ("src/test/java/RatioTest.java", JAVA_COMPUTED),
        ],
        ("kotlin", true) => vec![("src/main/kotlin/Ratio.kt", KOTLIN_COMPUTED)],
        ("kotlin", false) => vec![
            ("src/main/kotlin/Ratio.kt", KOTLIN_SAFE),
            ("src/test/kotlin/RatioTest.kt", KOTLIN_COMPUTED),
        ],
        ("csharp", true) => vec![("src/Ratio.cs", CSHARP_COMPUTED)],
        ("csharp", false) => vec![
            ("src/Ratio.cs", CSHARP_SAFE),
            ("tests/RatioTests.cs", CSHARP_COMPUTED),
        ],
        ("c", true) => vec![("src/ratio.c", C_COMPUTED)],
        ("c", false) => vec![("src/ratio.c", C_SAFE), ("tests/test_ratio.c", C_COMPUTED)],
        ("cpp", true) => vec![("src/ratio.cpp", C_COMPUTED)],
        ("cpp", false) => vec![
            ("src/ratio.cpp", C_SAFE),
            ("tests/ratio_test.cpp", C_COMPUTED),
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

const PYTHON_COMPUTED: &str = r#"def check(a, b, values):
    total = a + b
    if total == 0.3:
        return 1
    if a * 3 == 0.9:
        return 2
    acc = 0.0
    for value in values:
        acc += value
    return acc != 0.6
"#;

const PYTHON_SAFE: &str = r#"def check(a, config, values):
    if a == 0.1:
        return 1
    ratio = float(config["ratio"])
    if ratio == 0.1:
        return 2
    half = a / 2
    if half == 0.5:
        return 3
    count = len(values) + 1
    if count == 3:
        return 4
    return 0.1 == 0.1
"#;

const JS_COMPUTED: &str = r#"export function check(a, b, values) {
  const total = a + b;
  if (total === 0.3) return 1;
  if ((a - b) == 0.7) return 2;
  let acc = 0;
  for (const value of values) acc += value;
  return acc !== 0.6;
}
"#;

const JS_SAFE: &str = r#"export function check(a, config) {
  if (a === 0.1) return 1;
  const ratio = Number(config.ratio);
  if (ratio === 0.1) return 2;
  const half = a / 2;
  if (half === 0.5) return 3;
  const next = a + 1;
  if (next === 3) return 4;
  return 0;
}
"#;

const RUST_COMPUTED: &str = r#"pub fn check(a: f64, b: f64, values: &[f64]) -> u8 {
    let total = a + b;
    if total == 0.3 {
        return 1;
    }
    if a * 3.0 == 0.9 {
        return 2;
    }
    let mut acc = 0.0;
    for value in values {
        acc += value;
    }
    if acc != 0.6 {
        3
    } else {
        0
    }
}
"#;

const RUST_SAFE: &str = r#"pub fn check(a: f64, raw: &str) -> u8 {
    if a == 0.1 {
        return 1;
    }
    let ratio: f64 = raw.parse().unwrap_or(0.0);
    if ratio == 0.1 {
        return 2;
    }
    let half = a / 2.0;
    if half == 0.5 {
        return 3;
    }
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn sums_are_inexact() {
        let sum = 0.1 + 0.2;
        assert!(sum != 0.3);
    }
}
"#;

const GO_COMPUTED: &str = r#"package ratio

func Check(a, b float64, values []float64) int {
	total := a + b
	if total == 0.3 {
		return 1
	}
	if a*3 == 0.9 {
		return 2
	}
	acc := 0.0
	for _, value := range values {
		acc += value
	}
	if acc != 0.6 {
		return 3
	}
	return 0
}
"#;

const GO_SAFE: &str = r#"package ratio

import "strconv"

func Check(a float64, raw string) int {
	if a == 0.1 {
		return 1
	}
	ratio, _ := strconv.ParseFloat(raw, 64)
	if ratio == 0.1 {
		return 2
	}
	half := a / 2
	if half == 0.5 {
		return 3
	}
	return 0
}
"#;

const JAVA_COMPUTED: &str = r#"class Ratio {
    int check(double a, double b, double[] values) {
        double total = a + b;
        if (total == 0.3) {
            return 1;
        }
        if (a * 3 == 0.9) {
            return 2;
        }
        double acc = 0.0;
        for (double value : values) {
            acc += value;
        }
        return acc != 0.6 ? 3 : 0;
    }
}
"#;

const JAVA_SAFE: &str = r#"class Ratio {
    int check(double a, String raw) {
        if (a == 0.1) {
            return 1;
        }
        double ratio = Double.parseDouble(raw);
        if (ratio == 0.1) {
            return 2;
        }
        double half = a / 2;
        if (half == 0.5) {
            return 3;
        }
        return 0;
    }
}
"#;

/// Each positive reports exactly these comparisons, and its negative none.
const KOTLIN_COMPUTED: &str = r#"fun check(a: Double, b: Double, values: List<Double>): Int {
    val total = a + b
    if (total == 0.3) return 1
    if (a * 3 == 0.9) return 2
    var acc = 0.0
    for (value in values) acc += value
    if (acc != 0.6F) return 3
    return 0
}
"#;

const KOTLIN_SAFE: &str = r#"fun check(a: Double, raw: String): Int {
    if (a == 0.1) return 1
    val ratio = raw.toDouble()
    if (ratio == 0.1) return 2
    val half = a / 2
    if (half == 0.5) return 3
    return 0
}
"#;

const CSHARP_COMPUTED: &str = r#"class Ratio {
    int Check(double a, double b, double[] values) {
        double total = a + b;
        if (total == 0.3) return 1;
        if (a * 3 == 0.9) return 2;
        double acc = 0.0;
        foreach (var value in values) acc += value;
        if (acc != 0.6f) return 3;
        return 0;
    }
}
"#;

const CSHARP_SAFE: &str = r#"class Ratio {
    int Check(double a, decimal price, string raw) {
        if (a == 0.1) return 1;
        double ratio = double.Parse(raw);
        if (ratio == 0.1) return 2;
        decimal total = price + price;
        if (total == 0.3m) return 3;
        double half = a / 2;
        if (half == 0.5) return 4;
        return 0;
    }
}
"#;

const C_COMPUTED: &str = r#"int check(double a, double b, const double *values, int n) {
    double total = a + b;
    if (total == 0.3) return 1;
    if (a * 3 == 0.9f) return 2;
    double acc = 0.0;
    for (int i = 0; i < n; i++) acc += values[i];
    if (acc != 0.6L) return 3;
    return 0;
}
"#;

const C_SAFE: &str = r#"int check(double a, const char *raw) {
    if (a == 0.1) return 1;
    double ratio = atof(raw);
    if (ratio == 0.1) return 2;
    double half = a / 2;
    if (half == 0.5) return 3;
    if (half == 0x1.8p1) return 4;
    return 0;
}
"#;

#[test]
fn float_boundaries_report_exactly_the_computed_comparisons() {
    let python = ["total == 0.3", "a * 3 == 0.9", "acc != 0.6"];
    let js = ["total === 0.3", "(a - b) == 0.7", "acc !== 0.6"];
    let c_family = ["total == 0.3", "a * 3 == 0.9f", "acc != 0.6L"];
    let expected: &[(&str, &[&str])] = &[
        ("python", &python),
        ("javascript", &js),
        ("typescript", &js),
        ("rust", &["total == 0.3", "a * 3.0 == 0.9", "acc != 0.6"]),
        ("go", &["total == 0.3", "a*3 == 0.9", "acc != 0.6"]),
        ("java", &["total == 0.3", "a * 3 == 0.9", "acc != 0.6"]),
        ("kotlin", &["total == 0.3", "a * 3 == 0.9", "acc != 0.6F"]),
        ("csharp", &["total == 0.3", "a * 3 == 0.9", "acc != 0.6f"]),
        ("c", &c_family),
        ("cpp", &c_family),
    ];
    let mut failures = Vec::new();
    for (language, comparisons) in expected {
        for positive in [true, false] {
            let files = float_capability_files(DETECTOR, language, positive).unwrap();
            let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let reported = analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == DETECTOR)
                .map(|finding| {
                    finding
                        .message
                        .split('`')
                        .nth(1)
                        .unwrap_or_default()
                        .to_string()
                })
                .collect::<BTreeSet<_>>();
            let wanted = if positive {
                comparisons
                    .iter()
                    .map(|comparison| comparison.to_string())
                    .collect()
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
