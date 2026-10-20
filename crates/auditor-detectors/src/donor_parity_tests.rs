use std::collections::BTreeSet;

use super::agent_slop_disposition::{ParityRelation, AGENT_SLOP_DISPOSITIONS};

use super::oracle_runner::{
    assert_live_case, assert_precision_extension_noise, manifest_donor_ids, CapabilityStatus,
    OracleAdapter, ParityCase, DONOR_CAPABILITY_STATUSES,
};
use crate::ScanProfile;

#[test]
fn donor_capability_statuses_cover_the_manifest_without_claiming_unavailable_parity() {
    let manifest_ids = manifest_donor_ids();
    let status_ids = DONOR_CAPABILITY_STATUSES
        .iter()
        .map(|donor| donor.id.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(status_ids, manifest_ids);
    assert_eq!(DONOR_CAPABILITY_STATUSES.len(), 14);

    let mut live = 0;
    let mut not_parity = 0;
    for donor in DONOR_CAPABILITY_STATUSES {
        match donor.status {
            CapabilityStatus::LiveOracle { command } => {
                live += 1;
                assert!(
                    !command.trim().is_empty(),
                    "{} lacks a live command",
                    donor.id
                );
            }
            CapabilityStatus::NotParity {
                reason,
                eventual_command,
                capability_alternative,
            } => {
                not_parity += 1;
                assert!(
                    !reason.trim().is_empty(),
                    "{} lacks a non-parity reason",
                    donor.id
                );
                assert!(
                    !eventual_command.trim().is_empty(),
                    "{} lacks a future command",
                    donor.id
                );
                assert!(
                    !capability_alternative.trim().is_empty(),
                    "{} lacks a capability-test alternative",
                    donor.id
                );
            }
        }
    }
    assert_eq!((live, not_parity), (12, 2));
}

#[test]
fn agent_slop_dispositions_are_complete_unique_and_classified() {
    let rules = AGENT_SLOP_DISPOSITIONS
        .iter()
        .map(|item| item.donor_rule)
        .collect::<BTreeSet<_>>();
    assert_eq!(rules.len(), 24);
    assert_eq!(rules.len(), AGENT_SLOP_DISPOSITIONS.len());
    assert!(AGENT_SLOP_DISPOSITIONS
        .iter()
        .all(|item| !item.ea_detector.is_empty()));
    assert_eq!(
        AGENT_SLOP_DISPOSITIONS
            .iter()
            .filter(|item| item.relation == ParityRelation::PrecisionExtension)
            .map(|item| item.donor_rule)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "lexical.stutter",
            "structural.redundancy",
            "structural.types.sentinels",
        ])
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn pmd_live_oracle_matches_class_naming() {
    assert_live_case(
        OracleAdapter::Pmd,
        ParityCase {
            donor_id: "pmd",
            donor_rule: "ClassNamingConventions",
            ea_detector: "naming_convention",
            relative_path: "Sample.java",
            source: "public class sample {}\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn ruff_live_oracle_matches_function_complexity() {
    assert_live_case(
        OracleAdapter::Ruff,
        ParityCase {
            donor_id: "ruff",
            donor_rule: "C901",
            ea_detector: "function_complexity",
            relative_path: "source.py",
            source: "def complex_value(value):\n    result = value\n    if value > 0: result += 1\n    if value > 1: result += 1\n    if value > 2: result += 1\n    if value > 3: result += 1\n    if value > 4: result += 1\n    if value > 5: result += 1\n    if value > 6: result += 1\n    if value > 7: result += 1\n    if value > 8: result += 1\n    if value > 9: result += 1\n    if value > 10: result += 1\n    return result\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn ruff_live_oracle_matches_manual_list_copy() {
    assert_live_case(
        OracleAdapter::Ruff,
        ParityCase {
            donor_id: "ruff",
            donor_rule: "PERF402",
            ea_detector: "bulk_collection_extension",
            relative_path: "source.py",
            source: "def copy_values(items: list[int]):\n    result = []\n    for item in items:\n        result.append(item)\n    return result\n",
            profile: ScanProfile::Full,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn pylint_live_oracle_matches_excessive_nesting() {
    assert_live_case(
        OracleAdapter::Pylint,
        ParityCase {
            donor_id: "pylint",
            donor_rule: "too-many-nested-blocks",
            ea_detector: "excessive_nesting_depth",
            relative_path: "source.py",
            source: "def nested(a, b, c, d, e, f):\n    if a:\n        if b:\n            if c:\n                if d:\n                    if e:\n                        if f:\n                            return 1\n    return 0\n",
            profile: ScanProfile::Full,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn pylint_live_oracle_matches_generator_materialization() {
    assert_live_case(
        OracleAdapter::Pylint,
        ParityCase {
            donor_id: "pylint",
            donor_rule: "consider-using-generator",
            ea_detector: "intermediate_collection_materialization",
            relative_path: "source.py",
            source: "def total(items: list[int]):\n    return sum([item for item in items])\n",
            profile: ScanProfile::Full,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn eslint_live_oracle_matches_duplicate_imports() {
    assert_live_case(
        OracleAdapter::Eslint,
        ParityCase {
            donor_id: "eslint",
            donor_rule: "no-duplicate-imports",
            ea_detector: "duplicate_import",
            relative_path: "source.js",
            source: "import { alpha } from './model.js';\nimport { beta } from './model.js';\nconsume(alpha, beta);\n",
            profile: ScanProfile::Full,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn import_linter_live_oracle_matches_resolved_layer_contract() {
    assert_live_case(
        OracleAdapter::ImportLinter,
        ParityCase {
            donor_id: "import-linter",
            donor_rule: "forbidden-layer-contract",
            ea_detector: "architectural_layering",
            relative_path: "domain/user.py",
            source: "from app.service import UserService\n\ndef check():\n    return UserService\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn dependency_cruiser_live_oracle_matches_resolved_cycle() {
    assert_live_case(
        OracleAdapter::DependencyCruiser,
        ParityCase {
            donor_id: "dependency-cruiser",
            donor_rule: "no-circular",
            ea_detector: "architectural_layering",
            relative_path: "a.js",
            source: "// donor-cycle\nimport { b } from './b.js';\nexport const a = b;\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn jscpd_live_oracle_matches_minimum_token_clones() {
    assert_live_case(
        OracleAdapter::Jscpd,
        ParityCase {
            donor_id: "jscpd",
            donor_rule: "minimum-token-clone",
            ea_detector: "source_duplicate_detection",
            relative_path: "source.py",
            source: "def process_values(items, factor):\n    total = 0\n    total = total + items[0] * factor\n    total = total + items[1] * factor\n    total = total + items[2] * factor\n    total = total + items[3] * factor\n    total = total + items[4] * factor\n    total = total + items[5] * factor\n    total = total + items[6] * factor\n    total = total + items[7] * factor\n    return total\n\ndef aggregate_values(items, factor):\n    total = 0\n    total = total + items[0] * factor\n    total = total + items[1] * factor\n    total = total + items[2] * factor\n    total = total + items[3] * factor\n    total = total + items[4] * factor\n    total = total + items[5] * factor\n    total = total + items[6] * factor\n    total = total + items[7] * factor\n    return total\n",
            profile: ScanProfile::Full,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn bandit_live_oracle_matches_atomic_security_capabilities() {
    let cases = [
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B101",
            ea_detector: "assert_in_production",
            relative_path: "source.py",
            source: "def positive(value):\n    assert value > 0\n    return value\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B108",
            ea_detector: "hardcoded_tmp_directory",
            relative_path: "source.py",
            source: "temporary_path = '/tmp/unsafe'\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B103",
            ea_detector: "world_writable_file",
            relative_path: "source.py",
            source: "import os\nos.chmod('secret.txt', 0o777)\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B110",
            ea_detector: "try_except_pass",
            relative_path: "source.py",
            source: "try:\n    risky()\nexcept:\n    pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B301",
            ea_detector: "unsafe_deserialization",
            relative_path: "source.py",
            source: "import pickle\nvalue = pickle.loads(payload)\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B311",
            ea_detector: "random_for_security",
            relative_path: "source.py",
            source: "import random\ntoken = random.randint(0, 100)\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B501",
            ea_detector: "request_no_cert_verify",
            relative_path: "source.py",
            source: "import requests\nresponse = requests.get('https://example.com', verify=False)\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B507",
            ea_detector: "ssh_no_host_key_verification",
            relative_path: "source.py",
            source: "import paramiko\nclient = paramiko.SSHClient()\nclient.set_missing_host_key_policy(paramiko.AutoAddPolicy())\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "bandit",
            donor_rule: "B602",
            ea_detector: "shell_execution",
            relative_path: "source.py",
            source: "import subprocess\nsubprocess.call('echo unsafe', shell=True)\n",
            profile: ScanProfile::Gating,
        },
    ];
    for case in cases {
        assert_live_case(OracleAdapter::Bandit, case);
    }
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn agent_slop_live_oracle_matches_atomic_rule_capabilities() {
    let cases = [
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.local_imports",
            ea_detector: "function_local_import",
            relative_path: "source.py",
            source: "def platform_name():\n    import os\n    return os.name\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "information.section_comments",
            ea_detector: "section_divider_comment",
            relative_path: "source.py",
            source: "def process(value):\n    # --- validate ---\n    value += 1\n    # --- transform ---\n    value *= 2\n    # --- finalize ---\n    return value\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "information.magic_literals",
            ea_detector: "magic_literal_cluster",
            relative_path: "source.py",
            source: "def calculate(value):\n    alpha = value + 11\n    beta = alpha * 17\n    gamma = beta - 23\n    delta = gamma / 29\n    return delta\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "information.volume",
            ea_detector: "halstead_information_density",
            relative_path: "source.py",
            source: concat!(
                "def calculate(input_value):\n",
                "    total = input_value + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 + 11 + 12 + 13 + 14 + 15 + 16 + 17 + 18 + 19 + 20 + 21 + 22 + 23 + 24 + 25 + 26 + 27 + 28 + 29 + 30 + 31 + 32 + 33 + 34 + 35 + 36 + 37 + 38 + 39 + 40 + 41 + 42 + 43 + 44 + 45 + 46 + 47 + 48 + 49 + 50 + 51 + 52 + 53 + 54 + 55 + 56 + 57 + 58 + 59 + 60 + 61 + 62 + 63 + 64 + 65 + 66 + 67 + 68 + 69 + 70 + 71 + 72 + 73 + 74 + 75 + 76 + 77 + 78 + 79 + 80 + 81 + 82 + 83 + 84 + 85 + 86 + 87 + 88 + 89 + 90 + 91 + 92 + 93 + 94 + 95 + 96 + 97 + 98 + 99 + 100 + 101 + 102 + 103 + 104 + 105 + 106 + 107 + 108 + 109 + 110 + 111 + 112 + 113 + 114 + 115 + 116 + 117 + 118 + 119 + 120 + 121 + 122 + 123 + 124 + 125 + 126 + 127 + 128 + 129 + 130\n",
                "    return total\n",
            ),
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "lexical.verbosity",
            ea_detector: "identifier_verbosity",
            relative_path: "source.py",
            source: "def calculate_total_aggregated_value_with_scaling(input_data_collection, scaling_factor_parameter):\n    calculated_result_value_variable_with_extra_context = sum(input_data_collection) * scaling_factor_parameter\n    return calculated_result_value_variable_with_extra_context\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.god_module",
            ea_detector: "god_module",
            relative_path: "source.py",
            source: "def function_01(): pass\ndef function_02(): pass\ndef function_03(): pass\ndef function_04(): pass\ndef function_05(): pass\ndef function_06(): pass\ndef function_07(): pass\ndef function_08(): pass\ndef function_09(): pass\ndef function_10(): pass\ndef function_11(): pass\ndef function_12(): pass\ndef function_13(): pass\ndef function_14(): pass\ndef function_15(): pass\ndef function_16(): pass\ndef function_17(): pass\ndef function_18(): pass\ndef function_19(): pass\ndef function_20(): pass\ndef function_21(): pass\ndef function_22(): pass\ndef function_23(): pass\ndef function_24(): pass\ndef function_25(): pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.redundancy",
            ea_detector: "sibling_call_redundancy",
            relative_path: "source.py",
            source: "def first():\n    load(); validate(); persist()\ndef second():\n    load(); validate(); persist()\ndef load(): pass\ndef validate(): pass\ndef persist(): pass\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "lexical.stutter",
            ea_detector: "identifier_stutter",
            relative_path: "source.py",
            source: "def process_pdf_document(data):\n    pdf_document_bytes = 1\n    return data\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.complexity.cognitive",
            ea_detector: "cognitive_complexity",
            relative_path: "source.py",
            source: "def nested(a, b, c, d, e, f):\n    if a:\n        if b:\n            if c:\n                if d:\n                    if e:\n                        if f:\n                            return 1\n    return 0\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.complexity.cyclomatic",
            ea_detector: "function_complexity",
            relative_path: "source.py",
            source: "def branchy(a,b,c,d,e,f,g,h,i,j,k):\n    if a: pass\n    if b: pass\n    if c: pass\n    if d: pass\n    if e: pass\n    if f: pass\n    if g: pass\n    if h: pass\n    if i: pass\n    if j: pass\n    if k: pass\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.complexity.npath",
            ea_detector: "npath_complexity",
            relative_path: "source.py",
            source: "def paths(a,b,c,d,e,f,g,h,i,j):\n    if a: pass\n    if b: pass\n    if c: pass\n    if d: pass\n    if e: pass\n    if f: pass\n    if g: pass\n    if h: pass\n    if i: pass\n    if j: pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.types.sentinels",
            ea_detector: "stringly_typed_parameter",
            relative_path: "source.py",
            source: "def choose(mode: str):\n    if mode == 'fast':\n        return 1\n    if mode == 'safe':\n        return 2\n    return 0\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.types.hidden_mutators",
            ea_detector: "collection_parameter_mutation",
            relative_path: "source.py",
            source: "def collect_results(items: list[str]):\n    items.append('done')\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.types.escape_hatches",
            ea_detector: "escape_hatch_type_density",
            relative_path: "source.py",
            source: "from typing import Any, List, Dict\ndef process(data: Any) -> Any:\n    result: Any = {}\n    items: List[Any] = list(data)\n    output: Dict[str, Any] = {}\n    count: int = len(items)\n    return result\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "lexical.tersity",
            ea_detector: "identifier_tersity",
            relative_path: "source.py",
            source: "def terse(aa, bb, cc, dd, ee, gg):\n    hh = aa + bb\n    mm = cc * dd\n    rr = ee + gg + hh + mm\n    return rr\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.class.complexity",
            ea_detector: "class_structure_metrics",
            relative_path: "source.py",
            source: "class Complex:\n    def branchy(self, values):\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n        if values: pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.class.coupling",
            ea_detector: "class_structure_metrics",
            relative_path: "source.py",
            source: "class Dep0: pass\nclass Dep1: pass\nclass Dep2: pass\nclass Dep3: pass\nclass Dep4: pass\nclass Dep5: pass\nclass Dep6: pass\nclass Dep7: pass\nclass Dep8: pass\nclass Dep9: pass\nclass Hub:\n    def use(self, d0: Dep0, d1: Dep1, d2: Dep2, d3: Dep3, d4: Dep4, d5: Dep5, d6: Dep6, d7: Dep7, d8: Dep8, d9: Dep9): pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.class.inheritance.depth",
            ea_detector: "class_structure_metrics",
            relative_path: "source.py",
            source: "class A: pass\nclass B(A): pass\nclass C(B): pass\nclass D(C): pass\nclass E(D): pass\nclass F(E): pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.class.inheritance.children",
            ea_detector: "class_structure_metrics",
            relative_path: "source.py",
            source: "class Base: pass\nclass Child0(Base): pass\nclass Child1(Base): pass\nclass Child2(Base): pass\nclass Child3(Base): pass\nclass Child4(Base): pass\nclass Child5(Base): pass\nclass Child6(Base): pass\nclass Child7(Base): pass\nclass Child8(Base): pass\nclass Child9(Base): pass\nclass Child10(Base): pass\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "information.difficulty",
            ea_detector: "halstead_information_density",
            relative_path: "source.py",
            source: concat!(
                "def difficult(a, b):\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    x = a + b * a - b / a % b\n",
                "    y = (a == b) or (a != b) or (a < b) or (a > b) or (a <= b) or (a >= b)\n",
                "    return x if y else b\n",
            ),
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.duplication",
            ea_detector: "source_duplicate_detection",
            relative_path: "source.py",
            source: "def process_values(items, factor):\n    total = 0\n    total = total + items[0] * factor\n    total = total + items[1] * factor\n    total = total + items[2] * factor\n    total = total + items[3] * factor\n    total = total + items[4] * factor\n    total = total + items[5] * factor\n    total = total + items[6] * factor\n    total = total + items[7] * factor\n    return total\n\ndef aggregate_scores(entries, multiplier):\n    result = 0\n    result = result + entries[0] * multiplier\n    result = result + entries[1] * multiplier\n    result = result + entries[2] * multiplier\n    result = result + entries[3] * multiplier\n    result = result + entries[4] * multiplier\n    result = result + entries[5] * multiplier\n    result = result + entries[6] * multiplier\n    result = result + entries[7] * multiplier\n    return result\n",
            profile: ScanProfile::Full,
        },
    ];
    for case in cases {
        assert_live_case(OracleAdapter::AgentSlopLint, case);
    }
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn agent_slop_live_oracle_precision_extensions_reject_verified_donor_noise() {
    let cases = [
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "lexical.stutter",
            ea_detector: "identifier_stutter",
            relative_path: "source.py",
            source: "def repository_graph_lookup_with_mode():\n    return repository_graph_lookup_with_mode()\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.redundancy",
            ea_detector: "sibling_call_redundancy",
            relative_path: "source.py",
            source: "def first():\n    add(); append(); discard(); pop(); unique_first()\ndef second():\n    add(); append(); discard(); pop(); unique_second()\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.types.sentinels",
            ea_detector: "stringly_typed_parameter",
            relative_path: "source.py",
            source: "def choose(mode: str):\n    return mode\n",
            profile: ScanProfile::Gating,
        },
    ];
    for case in cases {
        assert_precision_extension_noise(OracleAdapter::AgentSlopLint, case);
    }
}

#[cfg(feature = "graph-analysis")]
#[test]
#[ignore = "requires the provisioned live donor lane"]
fn agent_slop_live_oracle_dependency_cycle_matches_authoritative_graph_detector() {
    assert_live_case(
        OracleAdapter::AgentSlopLint,
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.deps",
            ea_detector: "repo_graph_import_cycle",
            relative_path: "a.py",
            source: "import b\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn agent_slop_live_oracle_package_distance_matches_native_package_metrics() {
    assert_live_case(
        OracleAdapter::AgentSlopLint,
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.packages",
            ea_detector: "package_coupling_metrics",
            relative_path: "core/model.py",
            source: "class Model:\n    pass\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn agent_slop_live_oracle_hotspots_match_recent_growth_quadrants() {
    assert_live_case(
        OracleAdapter::AgentSlopLint,
        ParityCase {
            donor_id: "agent-slop-lint",
            donor_rule: "structural.hotspots",
            ea_detector: "churn_complexity_hotspot",
            relative_path: "hot.py",
            source: "def hot(a,b,c,d,e,f):\n    if a: pass\n    if b: pass\n    if c: pass\n    if d: pass\n    if e: pass\n    if f: pass\n",
            profile: ScanProfile::Gating,
        },
    );
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn clippy_live_oracle_matches_atomic_rust_capabilities() {
    let cases = [
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::too_many_arguments",
            ea_detector: "too_many_parameters",
            relative_path: "src/lib.rs",
            source: "pub fn wide(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) -> i32 {\n    a + b + c + d + e + f + g + h\n}\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::todo",
            ea_detector: "todo_unimplemented_in_production",
            relative_path: "src/lib.rs",
            source: "pub fn unfinished() {\n    todo!()\n}\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::panic",
            ea_detector: "unhandled_termination_in_production",
            relative_path: "src/lib.rs",
            source: "pub fn terminate() {\n    panic!(\"stop\")\n}\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::manual_memcpy",
            ea_detector: "manual_collection_copy_loop",
            relative_path: "src/lib.rs",
            source: "pub fn copy(source: &[u8], target: &mut [u8]) {\n    for i in 0..source.len() {\n        target[i] = source[i];\n    }\n}\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::or_fun_call",
            ea_detector: "eager_fallback_computation",
            relative_path: "src/lib.rs",
            source: "pub fn resolve(value: Option<String>) -> String {\n    value.unwrap_or(String::from(\"fallback\"))\n}\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::needless_collect",
            ea_detector: "intermediate_collection_materialization",
            relative_path: "src/lib.rs",
            source: "pub fn count(items: &[i32]) -> usize {\n    items.iter().filter(|item| **item > 0).collect::<Vec<_>>().len()\n}\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "clippy::regex_creation_in_loops",
            ea_detector: "loop_invariant_regex_compilation",
            relative_path: "src/lib.rs",
            source: "use regex::Regex;\npub fn matches(items: &[&str]) {\n    for item in items {\n        let pattern = Regex::new(\"^fixed$\").unwrap();\n        consume(pattern.is_match(item));\n    }\n}\nfn consume(value: bool) { let _ = value; }\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "rust-clippy",
            donor_rule: "unreachable_code",
            ea_detector: "unreachable_code",
            relative_path: "src/lib.rs",
            source: "pub fn evaluate() -> i32 {\n    return 1;\n    let value = 2;\n    value\n}\n",
            profile: ScanProfile::Full,
        },
    ];
    for case in cases {
        assert_live_case(OracleAdapter::RustClippy, case);
    }
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn golangci_live_oracle_matches_atomic_go_capabilities() {
    let cases = [
        ParityCase {
            donor_id: "golangci-lint",
            donor_rule: "errcheck",
            ea_detector: "go_errcheck",
            relative_path: "source.go",
            source: "package parity\n\nimport \"os\"\n\nfunc ignored(file *os.File) {\n    file.Close()\n}\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "golangci-lint",
            donor_rule: "ineffassign",
            ea_detector: "unused_assignment",
            relative_path: "source.go",
            source: "package parity\n\nfunc value() int {\n    result := 1\n    result = 2\n    return result\n}\n",
            profile: ScanProfile::Gating,
        },
        ParityCase {
            donor_id: "golangci-lint",
            donor_rule: "goconst",
            ea_detector: "go_repeated_string_literal",
            relative_path: "source.go",
            source: "package parity\n\nfunc label(code int) string {\n    if code == 1 {\n        return \"status-value\"\n    }\n    if code == 2 {\n        return \"status-value\"\n    }\n    return \"status-value\"\n}\n",
            profile: ScanProfile::Full,
        },
        ParityCase {
            donor_id: "golangci-lint",
            donor_rule: "prealloc",
            ea_detector: "known_size_preallocation",
            relative_path: "source.go",
            source: "package parity\n\nfunc copyValues(items []int) []int {\n    result := make([]int, 0)\n    for _, item := range items {\n        result = append(result, item)\n    }\n    return result\n}\n",
            profile: ScanProfile::Full,
        },
    ];
    for case in cases {
        assert_live_case(OracleAdapter::GolangCiLint, case);
    }
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn detekt_live_oracle_matches_cyclomatic_complexity() {
    let case = ParityCase {
        donor_id: "detekt",
        donor_rule: "detekt.complexity.CyclomaticComplexMethod",
        ea_detector: "function_complexity",
        relative_path: "source.kt",
        source: "fun complex(value: Int): Int {\n    var result = value\n    if (value > 0) result += 1\n    if (value > 1) result += 1\n    if (value > 2) result += 1\n    if (value > 3) result += 1\n    if (value > 4) result += 1\n    if (value > 5) result += 1\n    if (value > 6) result += 1\n    if (value > 7) result += 1\n    if (value > 8) result += 1\n    if (value > 9) result += 1\n    if (value > 10) result += 1\n    if (value > 11) result += 1\n    if (value > 12) result += 1\n    if (value > 13) result += 1\n    if (value > 14) result += 1\n    return result\n}\n",
        profile: ScanProfile::Gating,
    };
    assert_live_case(OracleAdapter::Detekt, case);
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn detekt_live_oracle_matches_range_iteration_capability() {
    let case = ParityCase {
        donor_id: "detekt",
        donor_rule: "detekt.performance.ForEachOnRange",
        ea_detector: "range_for_each_iteration",
        relative_path: "source.kt",
        source:
            "fun emit(limit: Int) {\n    (0 until limit).forEach { value -> println(value) }\n}\n",
        profile: ScanProfile::Full,
    };
    assert_live_case(OracleAdapter::Detekt, case);
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn detekt_live_oracle_matches_spread_materialization() {
    let case = ParityCase {
        donor_id: "detekt",
        donor_rule: "detekt.performance.SpreadOperator",
        ea_detector: "intermediate_collection_materialization",
        relative_path: "source.kt",
        source: "fun consume(vararg values: String) = Unit\nfun emit(items: List<String>) {\n    consume(*items.toTypedArray())\n}\n",
        profile: ScanProfile::Full,
    };
    assert_live_case(OracleAdapter::Detekt, case);
}

#[test]
#[ignore = "requires the provisioned live donor lane"]
fn detekt_live_oracle_matches_unreachable_code() {
    let case = ParityCase {
        donor_id: "detekt",
        donor_rule: "detekt.potential-bugs.UnreachableCode",
        ea_detector: "unreachable_code",
        relative_path: "source.kt",
        source: "fun evaluate() {\n    return\n    println(\"unreachable\")\n}\n",
        profile: ScanProfile::Full,
    };
    assert_live_case(OracleAdapter::Detekt, case);
}
