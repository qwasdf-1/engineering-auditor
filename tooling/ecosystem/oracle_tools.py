"""Pinned reference tools for the corpus oracles: provisioning and fact collection.

Each collector runs one tool over a checkout's files and returns
({path: [facts]}, failed paths). A function fact is {name, start, end,
metrics}; a diagnostic fact is {rule, line, message, value, name}.
"""

import json
import os
import re
import shutil
import subprocess
import tempfile
from collections import defaultdict
from pathlib import Path

from tooling.ecosystem import run

PYTHON_TOOLS = ("lizard==1.24.1", "ruff==0.17.0", "complexipy==8.0.1", "bandit==1.8.6")
GO_TOOLS = ("github.com/uudashr/gocognit/cmd/gocognit@v1.2.0", "github.com/kisielk/errcheck@v1.9.0",
            "github.com/jgautheron/goconst/cmd/goconst@v1.8.2")
NODE_TOOLS = ("eslint@9.39.1", "typescript-eslint@8.46.2", "typescript@5.9.3", "eslint-plugin-sonarjs@3.0.5",
              "jscpd@4.0.5")
RCA_VERSION = "0.0.25"
UV_URL = "https://github.com/astral-sh/uv/releases/download/0.9.5/uv-x86_64-unknown-linux-gnu.tar.gz"
JRE_URL = ("https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.8%2B9/"
           "OpenJDK21U-jre_x64_linux_hotspot_21.0.8_9.tar.gz")
PMD_VERSION = "7.16.0"
PMD_URL = f"https://github.com/pmd/pmd/releases/download/pmd_releases%2F{PMD_VERSION}/pmd-dist-{PMD_VERSION}-bin.zip"

JS_LANGUAGES = frozenset({"javascript", "typescript"})
TIMEOUT = 900

ESLINT_CONFIG = """\
import tseslint from "typescript-eslint";
import sonarjs from "eslint-plugin-sonarjs";
export default [{
  files: ["**/*.{js,cjs,mjs,jsx,ts,cts,mts,tsx}"],
  languageOptions: {
    parser: tseslint.parser,
    parserOptions: { ecmaVersion: "latest", sourceType: "module", ecmaFeatures: { jsx: true } },
  },
  plugins: { sonarjs },
  linterOptions: { reportUnusedDisableDirectives: "off", noInlineConfig: true },
  rules: {
    "no-console": "error",
    "no-duplicate-imports": ["error", { includeExports: false, allowSeparateTypeImports: true }],
    "max-depth": ["error", 5],
    "sonarjs/cognitive-complexity": ["error", 0],
    "no-await-in-loop": "error",
    "no-empty": "error",
  },
}];
"""

PMD_RULESET = """\
<?xml version="1.0"?>
<ruleset name="ea-oracle" xmlns="http://pmd.sourceforge.net/ruleset/2.0.0"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    xsi:schemaLocation="http://pmd.sourceforge.net/ruleset/2.0.0 https://pmd.sourceforge.io/ruleset_2_0_0.xsd">
  <description>Engineering Auditor corpus oracle rules.</description>
  <rule ref="category/java/codestyle.xml/FieldNamingConventions"/>
  <rule ref="category/java/codestyle.xml/LocalVariableNamingConventions"/>
  <rule ref="category/java/codestyle.xml/MethodNamingConventions"/>
  <rule ref="category/java/codestyle.xml/ClassNamingConventions"/>
  <rule ref="category/java/design.xml/NPathComplexity">
    <properties><property name="reportLevel" value="1"/></properties>
  </rule>
  <rule ref="category/java/design.xml/CognitiveComplexity">
    <properties><property name="reportLevel" value="1"/></properties>
  </rule>
</ruleset>
"""

# The pylint budgets match EA's structural_metrics.rs budgets.
RUFF_ARGS = ("check", "--no-cache", "--isolated", "--preview", "--output-format", "json", "--select",
             "S101,T201,T203,PLC0415,PLR0911,PLR0913,PLR0914,PLR0915,S110,S311,S602,S604,S605",
             "--config", "lint.pylint.max-args=5", "--config", "lint.pylint.max-returns=5",
             "--config", "lint.pylint.max-statements=50", "--config", "lint.pylint.max-locals=15")

LIZARD_SCRIPT = """\
import json, sys, lizard
analyzer = lizard.FileAnalyzer(lizard.get_extensions([]))
print(json.dumps({path: [{"name": function.name, "start": function.start_line, "end": function.end_line,
                          "metrics": {"ccn": function.cyclomatic_complexity,
                                      "params": function.parameter_count}}
                         for function in analyzer(path).function_list]
                  for path in sys.argv[1:]}))
"""


def oracle_root() -> Path:
    return Path(os.environ.get("EA_ORACLE_ROOT", run.REPO_ROOT / "target" / "oracles"))


def tool_path(*parts) -> Path:
    return oracle_root().joinpath(*parts)


def tool_env() -> dict:
    root = oracle_root()
    return {**os.environ, "JAVA_HOME": str(root / "jre"), "GOPATH": str(root / "gopath"),
            "GOCACHE": str(root / "gocache"), "GOFLAGS": "-mod=mod", "GOTOOLCHAIN": "local",
            "PATH": f"{root / 'jre' / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}"}


def command(argv, cwd, accept=(0,)) -> subprocess.CompletedProcess:
    result = subprocess.run([str(arg) for arg in argv], cwd=cwd, env=tool_env(), capture_output=True,
                            text=True, timeout=TIMEOUT)
    if result.returncode not in accept:
        raise RuntimeError(f"{Path(str(argv[0])).name} exited {result.returncode}: "
                           f"{(result.stderr or result.stdout).strip()[-400:]}")
    return result


def tool(relative, *args, cwd, accept=(0,)) -> subprocess.CompletedProcess:
    """Run an installed oracle, named by its path under the oracle root."""
    return command([tool_path(*relative.split("/")), *args], cwd, accept)


# Provisioning ------------------------------------------------------------------------------

def provision() -> None:
    root = oracle_root()
    downloads = root / "dl"
    downloads.mkdir(parents=True, exist_ok=True)
    uv = root / "bin" / "uv"
    if not uv.exists():
        command(["curl", "-sSfL", "-o", downloads / "uv.tgz", UV_URL], root)
        command(["tar", "xzf", downloads / "uv.tgz", "-C", downloads], root)
        uv.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(downloads / "uv-x86_64-unknown-linux-gnu" / "uv", uv)
    uv_env = ["env", f"UV_CACHE_DIR={root / 'uv-cache'}"]
    if not (root / "py" / "bin" / "python").exists():
        command([*uv_env, uv, "venv", "-q", "--python", "3.12", root / "py"], root)
    command([*uv_env, uv, "pip", "install", "-q", "--python", root / "py" / "bin" / "python", *PYTHON_TOOLS], root)
    for package in GO_TOOLS:
        command(["env", f"GOBIN={root / 'bin'}", "go", "install", package], root)
    (root / "node").mkdir(exist_ok=True)
    if not (root / "node" / "package.json").exists():
        (root / "node" / "package.json").write_text('{"private": true}\n', encoding="utf-8")
    command(["npm", "install", "-s", "--no-fund", "--no-audit", *NODE_TOOLS], root / "node")
    command(["env", f"CARGO_TARGET_DIR={root / 'cargo-target'}", "cargo", "install", "-q", "--locked",
             "--root", root, f"rust-code-analysis-cli@{RCA_VERSION}"], root)
    if not (root / "jre" / "bin" / "java").exists():
        command(["curl", "-sSfL", "-o", downloads / "jre.tgz", JRE_URL], root)
        (root / "jre").mkdir(exist_ok=True)
        command(["tar", "xzf", downloads / "jre.tgz", "-C", root / "jre", "--strip-components=1"], root)
    if not (root / f"pmd-bin-{PMD_VERSION}").exists():
        command(["curl", "-sSfL", "-o", downloads / "pmd.zip", PMD_URL], root)
        shutil.unpack_archive(downloads / "pmd.zip", root)
        (root / f"pmd-bin-{PMD_VERSION}" / "bin" / "pmd").chmod(0o755)
    print(json.dumps(versions(), indent=2))


def versions() -> dict:
    """Installed oracle versions; a check's source names the version that made it."""
    return {
        "lizard": PYTHON_TOOLS[0].split("==")[1], "ruff": PYTHON_TOOLS[1].split("==")[1],
        "complexipy": PYTHON_TOOLS[2].split("==")[1], "bandit": PYTHON_TOOLS[3].split("==")[1],
        "gocognit": GO_TOOLS[0].rsplit("@", 1)[1], "errcheck": GO_TOOLS[1].rsplit("@", 1)[1],
        "goconst": GO_TOOLS[2].rsplit("@", 1)[1],
        "eslint": NODE_TOOLS[0].split("@")[1], "jscpd": NODE_TOOLS[4].split("@")[1],
        "rca": RCA_VERSION, "pmd": PMD_VERSION, "nonblank": "1",
    }


# Fact collection ---------------------------------------------------------------------------
# Each collector takes a checkout and the relative paths to analyze and returns
# ({path: [fact]}, failed paths). A function fact is {name, start, end, metrics};
# a diagnostic is {line, rule, message, value}.

def grouped(facts) -> dict:
    by_path = defaultdict(list)
    for fact in facts:
        by_path[fact.pop("path")].append(fact)
    return dict(by_path)


def number_in(message, pattern=r"(\d+(?:\.\d+)?)"):
    match = re.search(pattern, message or "")
    return float(match.group(1)) if match else None


def lizard_facts(checkout, paths):
    result = tool("py/bin/python", "-c", LIZARD_SCRIPT, *paths, cwd=checkout)
    return json.loads(result.stdout), set()


def rca_spaces(space, out):
    for child in space.get("spaces", []):
        if child.get("kind") == "function":
            metrics = child.get("metrics", {})
            out.append({"name": child.get("name") or "", "start": child["start_line"], "end": child["end_line"],
                        "metrics": {"cognitive": metrics["cognitive"]["sum"], "nexits": metrics["nexits"]["sum"],
                                    "cyclomatic": metrics["cyclomatic"]["sum"],
                                    "halstead_volume": metrics["halstead"]["volume"],
                                    "halstead_difficulty": metrics["halstead"]["difficulty"]}})
        rca_spaces(child, out)


def rca_facts(checkout, paths):
    facts, failed = {}, set()
    for path in paths:
        try:
            result = tool("bin/rust-code-analysis-cli", "-m", "-O", "json", "-p", path, cwd=checkout)
            spaces = []
            rca_spaces(json.loads(result.stdout), spaces)
            facts[path] = spaces
        except (RuntimeError, ValueError, KeyError):
            failed.add(path)
    return facts, failed


def complexipy_facts(checkout, paths):
    with tempfile.TemporaryDirectory() as scratch:
        report = Path(scratch) / "complexipy.json"
        tool("py/bin/complexipy", *paths, "--output-format", "json", "--output", report, "-q", "--cache-dir",
             Path(scratch) / "cache", cwd=checkout, accept=(0, 1))
        rows = json.loads(report.read_text(encoding="utf-8"))
    return grouped({"path": row["path"], "name": row["function_name"], "start": None, "end": None,
                    "metrics": {"cognitive": row["complexity"]}} for row in rows), set()


def gocognit_facts(checkout, paths):
    result = tool("bin/gocognit", "-over", "0", *paths, cwd=checkout)
    facts = []
    for line in result.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 4:
            path, start = parts[3].rsplit(":", 2)[:2]
            facts.append({"path": os.path.normpath(path), "name": parts[2], "start": int(start), "end": None,
                          "metrics": {"cognitive": int(parts[0])}})
    return grouped(facts), set()


def ruff_facts(checkout, paths):
    result = tool("py/bin/ruff", *RUFF_ARGS, *paths, cwd=checkout, accept=(0, 1))
    facts, failed = [], set()
    for item in json.loads(result.stdout):
        path = os.path.relpath(item["filename"], checkout)
        if item["code"] is None:  # syntax error: the file was not analyzed
            failed.add(path)
            continue
        facts.append({"path": path, "line": item["location"]["row"], "rule": item["code"],
                      "message": item["message"], "value": number_in(item["message"], r"\((\d+) >")})
    return grouped(facts), failed


def bandit_facts(checkout, paths):
    """Bandit's B113 (`requests`/`httpx` without a timeout), `# nosec` ignored."""
    result = tool("py/bin/bandit", "-q", "-f", "json", "-t", "B113", "--ignore-nosec", *paths,
                  cwd=checkout, accept=(0, 1))
    report = json.loads(result.stdout)
    failed = {os.path.relpath(error["filename"], checkout) for error in report.get("errors", [])}
    facts = [{"path": os.path.relpath(item["filename"], checkout), "line": item["line_number"],
              "rule": item["test_id"], "message": item["issue_text"], "value": None}
             for item in report.get("results", [])]
    return grouped(facts), failed


def eslint_config() -> Path:
    """Write the pinned flat config beside the installed plugins it imports."""
    config = tool_path("node", "ea-oracle.config.mjs")
    config.write_text(ESLINT_CONFIG, encoding="utf-8")
    return config


def eslint_facts(checkout, paths):
    result = tool("node/node_modules/.bin/eslint", "--no-config-lookup", "-c", eslint_config(), "--format", "json",
                  *paths, cwd=checkout, accept=(0, 1))
    facts, failed = [], set()
    for report in json.loads(result.stdout):
        path = os.path.relpath(report["filePath"], checkout)
        if report.get("fatalErrorCount"):
            failed.add(path)
            continue
        for message in report["messages"]:
            if message.get("ruleId"):
                facts.append({"path": path, "line": message["line"], "rule": message["ruleId"],
                              "message": message["message"], "value": number_in(message["message"])})
    return grouped(facts), failed


def module_root(checkout: Path, path: str) -> Path:
    directory = (checkout / path).parent
    while directory != checkout and not (directory / "go.mod").exists():
        directory = directory.parent
    return directory


def errcheck_facts(checkout, paths):
    facts, failed = [], set()
    modules = defaultdict(set)
    for path in paths:
        modules[module_root(checkout, path)].add(path)
    for module, members in sorted(modules.items()):
        packages = sorted({"./" + os.path.relpath((checkout / path).parent, module) for path in members})
        try:
            result = tool("bin/errcheck", "-ignoretests", *packages, cwd=module, accept=(0, 1))
        except RuntimeError:
            failed |= members
            continue
        for line in result.stdout.splitlines():
            location, _, code = line.partition("\t")
            parts = location.rstrip(":").rsplit(":", 2)
            if len(parts) == 3:
                facts.append({"path": os.path.relpath(module / parts[0], checkout), "line": int(parts[1]),
                              "rule": "errcheck", "message": code.strip(), "value": None})
    return grouped(facts), failed


def goconst_facts(checkout, paths):
    packages = sorted({"./" + os.path.dirname(path) for path in paths})
    result = tool("bin/goconst", "-output", "json", "-min-occurrences", "3", "-min-length", "4", *packages,
                  cwd=checkout)
    # goconst may print one JSON document per batch of files.
    documents = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
    facts = []
    for literal, positions in ((k, v) for d in documents for k, v in d.get("strings", {}).items()):
        for position in positions:
            facts.append({"path": os.path.normpath(position["Filename"]), "line": position["Line"],
                          "rule": "goconst", "message": literal, "value": None})
    return grouped(facts), set()


def pmd_subject(description) -> str:
    """The quoted method or parameter name a PMD message is about."""
    match = re.search(r"'([^'(]+)", description)
    return match.group(1) if match else ""


def pmd_facts(checkout, paths):
    with tempfile.TemporaryDirectory() as scratch:
        ruleset, file_list = Path(scratch) / "ruleset.xml", Path(scratch) / "files.txt"
        ruleset.write_text(PMD_RULESET, encoding="utf-8")
        file_list.write_text("\n".join(paths) + "\n", encoding="utf-8")
        result = tool(f"pmd-bin-{PMD_VERSION}/bin/pmd", "check", "--no-cache", "--no-progress", "-f", "json", "-R",
                      ruleset, "--file-list", file_list, cwd=checkout, accept=(0, 4))
    report = json.loads(result.stdout)
    failed = {os.path.normpath(error["filename"]) for error in report.get("processingErrors", [])}
    facts = [{"path": os.path.normpath(file["filename"]), "line": violation["beginline"], "rule": violation["rule"],
              "message": violation["description"],
              "name": pmd_subject(violation["description"]),
              "value": number_in(violation["description"], r"complexity of (\d+)")}
             for file in report["files"] for violation in file["violations"]]
    return grouped(facts), failed


def nonblank_facts(checkout, paths):
    return {path: [{"lines": sum(1 for line in (checkout / path).read_text(encoding="utf-8", errors="replace")
                                  .splitlines() if line.strip())}] for path in paths}, set()


def jscpd_facts(checkout, paths):
    with tempfile.TemporaryDirectory() as scratch:
        tool("node/node_modules/.bin/jscpd", "--silent", "--reporters", "json", "--output", scratch,
             "--min-tokens", "50", "--min-lines", "8", *paths, cwd=checkout)
        report = Path(scratch) / "jscpd-report.json"
        duplicates = json.loads(report.read_text(encoding="utf-8"))["duplicates"] if report.exists() else []
    pairs = [((os.path.normpath(d["firstFile"]["name"]), d["firstFile"]["start"], d["firstFile"]["end"]),
              (os.path.normpath(d["secondFile"]["name"]), d["secondFile"]["start"], d["secondFile"]["end"]))
             for d in duplicates]
    return {path: pairs for path in paths}, set()


COLLECTORS = {
    "lizard": lizard_facts, "rca": rca_facts, "complexipy": complexipy_facts, "gocognit": gocognit_facts,
    "ruff": ruff_facts, "eslint": eslint_facts, "errcheck": errcheck_facts, "goconst": goconst_facts,
    "pmd": pmd_facts, "nonblank": nonblank_facts, "jscpd": jscpd_facts, "bandit": bandit_facts,
}


# Judging -----------------------------------------------------------------------------------
# A judge takes (entry, finding, facts for the entry's file) and returns
# (claim, detail), or None when the oracle cannot tell.
