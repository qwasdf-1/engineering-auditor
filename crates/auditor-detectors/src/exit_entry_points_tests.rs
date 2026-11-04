use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast_scan::AstScan;
use crate::{util, AnalysisMethod, AuditedFile, Language, SourceRole};

/// The lines of the exit findings in one file written at `rel`.
fn exit_lines(rel: &str, source: &str) -> Vec<usize> {
    exit_sites(&[(rel, source)])
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

/// `(path, line)` of the exit findings in the files written at their paths.
fn exit_sites(sources: &[(&str, &str)]) -> Vec<(String, usize)> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "engineering-auditor-exit-entry-{}-{nanos}",
        std::process::id()
    ));
    let files = sources
        .iter()
        .map(|(rel, source)| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, source).unwrap();
            let language = match rel.rsplit('.').next() {
                Some("go") => Language::Go,
                Some("ts") => Language::TypeScript,
                Some("js") => Language::JavaScript,
                Some("java") => Language::Java,
                _ => Language::Python,
            };
            AuditedFile {
                absolute: path,
                relative: std::path::PathBuf::from(rel),
                language,
                bytes: source.len() as u64,
                role: SourceRole::Production,
            }
        })
        .collect::<Vec<_>>();
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let structure = crate::structure_index::StructureIndex::build(AstScan::context(
        &files,
        AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let scan = AstScan::new(&files, AnalysisMethod::Ast, &cache, &mut repo_ast);
    let findings = super::super::exit_in_production_findings(scan, &structure);
    fs::remove_dir_all(&root).unwrap();
    let mut sites = findings
        .iter()
        .map(|finding| {
            (
                finding.primary_path().to_string(),
                finding.primary_line().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    sites.sort();
    sites
}

/// A library function body that exits; reported unless the shape owns it.
const LIB: &str = "import sys\n\ndef helper():\n    sys.exit(1)\n";

#[test]
fn an_exit_in_an_ordinary_library_function_is_reported() {
    assert_eq!(exit_lines("pkg/util.py", LIB), [4]);
    let java = "class Util {\n  void stop() {\n    System.exit(1);\n  }\n  void handleExitCode() {\n    System.exit(2);\n  }\n}\n";
    assert_eq!(exit_lines("src/Util.java", java), [3, 6]);
    let js = "export function stop() {\n  process.exit(1);\n}\n";
    assert_eq!(exit_lines("lib/util.ts", js), [2]);
    let go = "package util\nimport \"os\"\nfunc Stop() {\n\tos.Exit(1)\n}\nfunc exitStatus() {\n\tos.Exit(2)\n}\n";
    assert_eq!(exit_lines("util/util.go", go), [4, 7]);
}

#[test]
fn cli_command_handlers_own_their_exits() {
    let source = r#"import sys
import click
from click import group

@click.command()
@click.option("--x")
def migrate(x):
    def inner():
        sys.exit(1)
    sys.exit(0)

@click.group()
def tools():
    sys.exit(2)

@group
def more():
    sys.exit(3)

@manager.command()
def test():
    exit(1)

@app.command
def serve():
    exit(1)
"#;
    assert!(exit_lines("pkg/commands.py", source).is_empty());
}

#[test]
fn python_functions_with_other_decorators_are_reported() {
    let source = r#"import sys

@app.route("/stop")
def stop():
    sys.exit(1)

class Thing:
    @property
    def value(self):
        sys.exit(2)

    @staticmethod
    def make():
        sys.exit(3)
"#;
    assert_eq!(exit_lines("pkg/web.py", source), [5, 10, 14]);
}

#[test]
fn management_command_methods_own_their_exits() {
    let source = r#"import signal
import sys
from django.core.management.base import BaseCommand
from app.lib import AppBaseCommand

class Command(AppBaseCommand):
    def handle(self, *args, **options):
        def on_term(sig, frame):
            sys.exit(3)
        sys.exit(0)

class Other(BaseCommand):
    def handle(self):
        sys.exit(1)
"#;
    assert!(exit_lines("app/management/commands/job.py", source).is_empty());
    assert_eq!(exit_lines("app/jobs/job.py", source), [9, 10, 14]);
}

#[test]
fn classes_not_deriving_from_a_command_in_a_management_file_are_reported() {
    let source = "import sys\n\nclass Helper(Mixin):\n    def run(self):\n        sys.exit(1)\n";
    assert_eq!(exit_lines("app/management/commands/job.py", source), [5]);
}

#[test]
fn signal_handlers_the_function_installs_own_their_exits() {
    let source = r#"import signal
import sys

class Worker:
    def before_run(self):
        def receive(number, frame):
            sys.exit()
        signal.signal(signal.SIGTERM, receive)
        signal.signal(signal.SIGINT, lambda n, f: sys.exit(0))
        signal.signal(signal.SIGHUP, self.hang_up)

    def hang_up(self, number, frame):
        sys.exit(1)

    def other(self):
        sys.exit(2)
"#;
    assert_eq!(exit_lines("pkg/worker.py", source), [16]);
}

#[test]
fn a_function_not_registered_in_the_same_scope_is_not_a_signal_handler() {
    let source = r#"import signal
import sys

def dump(sig=None, frame=None):
    sys.exit(1)

def register():
    signal.signal(signal.SIGUSR2, dump)
"#;
    assert_eq!(exit_lines("pkg/debug.py", source), [5]);
}

#[test]
fn js_signal_handlers_own_their_exits() {
    let source = r#"export class Server {
  start() {
    process
      .removeAllListeners('SIGINT')
      .on('SIGINT', () => this.shutdown())
      .on('SIGTERM', () => {
        process.exit(0);
      });
    process.once('SIGHUP', this.reload);
    const bye = () => process.exit(4);
    process.on('SIGUSR2', bye);
  }

  async shutdown() {
    process.exit(0);
  }

  reload() {
    process.exit(3);
  }

  other() {
    process.exit(5);
  }
}
"#;
    assert_eq!(exit_lines("server/server.ts", source), [23]);
}

#[test]
fn js_listeners_for_other_events_are_not_signal_handlers() {
    let source = r#"export function install() {
  process.on('exit', () => {
    process.exit(1);
  });
  process.on('message', function (m) {
    process.exit(2);
  });
}
"#;
    assert_eq!(exit_lines("lib/install.js", source), [3, 6]);
}

#[test]
fn only_os_exit_in_a_fork_child_is_exempt() {
    let source = r#"import os
import sys

def open_url(url):
    if os.fork() == 0:
        os._exit(0)
    sys.exit(1)

def spawn():
    pid = os.fork()
    if pid == 0:
        try:
            run()
        finally:
            os._exit(0)
    else:
        os._exit(2)

def child_with_sys_exit():
    if os.fork() == 0:
        sys.exit(0)

def plain():
    os._exit(3)
"#;
    assert_eq!(exit_lines("pkg/proc.py", source), [7, 17, 21, 24]);
}

#[test]
fn multiprocessing_process_run_methods_own_their_exits() {
    let source = r#"import multiprocessing as mp
import sys
import threading
from multiprocessing import Process

class Direct(mp.Process):
    def run(self):
        sys.exit(1)
    def helper(self):
        sys.exit(2)

class Imported(Process):
    def run(self):
        sys.exit(3)

class Derived(Direct):
    def run(self):
        sys.exit(4)

class Tagged(Elsewhere):
    def run(self):
        sys.exit(5)

class Worker(threading.Thread):
    def run(self):
        sys.exit(6)
"#;
    assert_eq!(exit_lines("pkg/procs.py", source), [10, 22, 26]);
}

#[test]
fn functions_that_say_they_exit_own_their_exits() {
    let java = "class Cli {\n  void exit(int code) {\n    System.exit(code);\n  }\n  void printAndExit() {\n    System.exit(0);\n  }\n  void exitCode() {\n    System.exit(2);\n  }\n}\n";
    assert_eq!(exit_lines("src/Cli.java", java), [9]);
    let python = "import sys\n\ndef print_and_exit():\n    sys.exit(0)\n\ndef handle_exit_code():\n    sys.exit(1)\n";
    assert_eq!(exit_lines("pkg/cli.py", python), [7]);
    let go = "package server\nimport \"os\"\nfunc PrintServerAndExit() {\n\tos.Exit(0)\n}\nfunc (t *T) Exit() {\n\tos.Exit(0)\n}\nfunc PrintAndDie() {\n\tos.Exit(1)\n}\n";
    assert_eq!(exit_lines("server/server.go", go), [10]);
}

#[test]
fn go_urfave_cli_actions_own_their_exits() {
    let source = r#"package cmd

import (
	"context"
	"os"

	"github.com/urfave/cli/v3"
	old "github.com/urfave/cli"
	other "example.com/cli"
)

func runHook(_ context.Context, c *cli.Command) error {
	os.Exit(1)
	return nil
}

func runOld(c *old.Context) error {
	os.Exit(1)
	return nil
}

func run(c *other.Context) error {
	os.Exit(1)
	return nil
}

func plain(c *cli.Flag) {
	os.Exit(1)
}

var action = func(c *cli.Context) error {
	os.Exit(2)
	return nil
}
"#;
    assert_eq!(exit_lines("cmd/hook.go", source), [23, 28]);
}

#[test]
fn go_test_runners_own_their_exits_but_test_functions_do_not() {
    let source = r#"package git

import (
	"os"
	"testing"
)

func RunTests(m interface{ Run() int }) {
	os.Exit(m.Run())
}

func MainTest(m *testing.M) {
	os.Exit(m.Run())
}

func Helper(t *testing.T) {
	os.Exit(1)
}

func Other(m interface{ Stop() }) {
	os.Exit(1)
}
"#;
    assert_eq!(exit_lines("git/git.go", source), [17, 21]);
}

#[test]
fn process_run_methods_own_their_exits_through_bases_in_other_files() {
    let sites = exit_sites(&[
        (
            "app/util/process.py",
            "import multiprocessing as mp\n\n\nclass BaseProcess(mp.Process):\n    pass\n\n\nclass AppProcess(BaseProcess):\n    pass\n",
        ),
        (
            "app/embeddings.py",
            "import sys\n\nfrom app.util.process import AppProcess\n\n\nclass EmbeddingProcess(AppProcess):\n    def run(self):\n        sys.exit(1)\n\n    def stop(self):\n        sys.exit(2)\n",
        ),
    ]);
    assert_eq!(sites, [("app/embeddings.py".to_string(), 11)]);
}
