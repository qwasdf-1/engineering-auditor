use std::collections::BTreeSet;
#[cfg(feature = "graph-analysis")]
use std::path::Path;
use std::path::PathBuf;

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, ChangedRange, ScanProfile, ScanScope, SourceDiscoveryOptions,
};
use auditor_report::{ColorMode, ExternalInput, ExternalInputMetadata};

use crate::arg_cursor::ArgCursor;

pub(super) struct PreparedScan {
    pub request: AnalysisRequest,
    pub external_inputs: Vec<ExternalInput>,
}

pub(super) struct ScanOptions {
    pub repo_root: PathBuf,
    pub format: String,
    pub method: AnalysisMethod,
    pub baseline: Option<PathBuf>,
    pub output: Option<PathBuf>,
    /// `--no-color`: forces [`ColorMode::Plain`] regardless of tty detection.
    pub no_color: bool,
    /// `--quiet`: suppresses stdout; scan emits at most one terminal summary
    /// line on stderr. Task 13 wires the behavior end-to-end.
    pub quiet: bool,
    /// `--summary-only`: restricts `--format human` digest to sections 1-3 +
    /// footer. Errors when combined with `--format json`/`--format sarif`.
    pub summary_only: bool,
    profile: String,
    include_references: bool,
    paths: BTreeSet<String>,
    changed_ranges_json: Option<String>,
    #[cfg(feature = "graph-analysis")]
    pub repo_graph: Option<PathBuf>,
    #[cfg(feature = "graph-analysis")]
    pub co_edit_journal: Option<PathBuf>,
}

impl ScanOptions {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut cursor = ArgCursor::new(args);
        while let Some(argument) = cursor.next() {
            options.consume(argument, &mut cursor)?;
        }
        Ok(options)
    }

    /// Resolve the [`ColorMode`] for a scan run. Emitted color is allowed
    /// only when no `--output` file is set (stdout is a tty) and `--no-color`
    /// is absent. File destinations are not ttys, so `--output <path>`
    /// suppresses ANSI regardless of `--no-color`.
    pub(super) fn resolved_color(&self) -> ColorMode {
        if self.no_color {
            return ColorMode::Plain;
        }
        match self.output.as_deref() {
            Some(_) => ColorMode::Plain,
            None => {
                use std::io::IsTerminal;
                if std::io::stdout().is_terminal() {
                    ColorMode::Auto
                } else {
                    ColorMode::Plain
                }
            }
        }
    }

    pub fn prepare_with_exclusions(
        &self,
        additional_excluded_paths: &[PathBuf],
    ) -> Result<PreparedScan, String> {
        let profile = ScanProfile::parse(&self.profile)?;
        let scope = self.scan_scope()?;
        let external_inputs = self.external_inputs(profile, &scope)?;
        let mut excluded_paths = self.output.iter().cloned().collect::<BTreeSet<_>>();
        excluded_paths.extend(additional_excluded_paths.iter().cloned());
        let request = AnalysisRequest {
            discovery: SourceDiscoveryOptions {
                include_references: self.include_references,
                method: self.method,
            },
            profile,
            scope: scope.clone(),
            repository_map_output: self.repository_map_output(profile, &scope),
            excluded_paths,
        };
        Ok(PreparedScan {
            request,
            external_inputs,
        })
    }

    fn consume(&mut self, argument: &str, cursor: &mut ArgCursor<'_>) -> Result<(), String> {
        if self.consume_flag(argument) {
            return Ok(());
        }
        if self.consume_scope_argument(argument, cursor)?
            || self.consume_graph_argument(argument, cursor)?
        {
            return Ok(());
        }
        match argument {
            "--repo-root" => self.repo_root = PathBuf::from(cursor.required(argument)?),
            "--profile" => self.profile = cursor.required(argument)?.to_string(),
            "--format" => self.format = cursor.required(argument)?.to_string(),
            "--method" => {
                self.method = AnalysisMethod::parse(cursor.required(argument)?)
                    .map_err(|_| "--method requires auto|ast|token".to_string())?
            }
            "--baseline" => self.baseline = Some(PathBuf::from(cursor.required(argument)?)),
            "--output" => self.output = Some(PathBuf::from(cursor.required(argument)?)),
            other => return Err(format!("unknown scan argument `{other}`")),
        }
        Ok(())
    }

    fn consume_flag(&mut self, argument: &str) -> bool {
        match argument {
            "--include-references" => self.include_references = true,
            "--no-color" => self.no_color = true,
            "--quiet" => self.quiet = true,
            "--summary-only" => self.summary_only = true,
            _ => return false,
        }
        true
    }

    fn consume_scope_argument(
        &mut self,
        argument: &str,
        cursor: &mut ArgCursor<'_>,
    ) -> Result<bool, String> {
        match argument {
            "--path" => {
                self.paths.insert(cursor.required(argument)?.to_string());
                Ok(true)
            }
            "--changed-ranges-json" => {
                self.changed_ranges_json = Some(cursor.required(argument)?.to_string());
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    #[cfg(feature = "graph-analysis")]
    fn consume_graph_argument(
        &mut self,
        argument: &str,
        cursor: &mut ArgCursor<'_>,
    ) -> Result<bool, String> {
        match argument {
            "--repo-graph" => {
                self.repo_graph = Some(PathBuf::from(cursor.required(argument)?));
                Ok(true)
            }
            "--co-edit-journal" => {
                self.co_edit_journal = Some(PathBuf::from(cursor.required(argument)?));
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    #[cfg(not(feature = "graph-analysis"))]
    fn consume_graph_argument(
        &mut self,
        _argument: &str,
        _cursor: &mut ArgCursor<'_>,
    ) -> Result<bool, String> {
        Ok(false)
    }

    fn scan_scope(&self) -> Result<ScanScope, String> {
        let ranges = parse_changed_ranges(self.changed_ranges_json.as_deref())?;
        let mut paths = self.paths.clone();
        paths.extend(ranges.iter().map(|range| range.path.clone()));
        if paths.is_empty() && ranges.is_empty() {
            Ok(ScanScope::All)
        } else {
            Ok(ScanScope::Changed { paths, ranges })
        }
    }

    fn external_inputs(
        &self,
        profile: ScanProfile,
        scope: &ScanScope,
    ) -> Result<Vec<ExternalInput>, String> {
        let request = format!(
            "profile={};method={};include_references={};scope={scope:?}",
            profile.as_str(),
            self.method.as_str(),
            self.include_references,
        );
        let mut inputs = vec![ExternalInput::from_text(
            ExternalInputMetadata::analysis("scan_request", "cli", 1, "complete"),
            &request,
        )];
        if let Some(changed) = &self.changed_ranges_json {
            inputs.push(ExternalInput::from_text(
                ExternalInputMetadata::supplemental("changed_ranges", "cli", 1, "complete"),
                changed,
            ));
        }
        #[cfg(feature = "graph-analysis")]
        self.add_graph_inputs(&mut inputs)?;
        Ok(inputs)
    }

    fn repository_map_output(&self, profile: ScanProfile, scope: &ScanScope) -> Option<PathBuf> {
        (profile == ScanProfile::Full
            && matches!(scope, ScanScope::All)
            && self.format == "json"
            && self.method != AnalysisMethod::Token)
            .then(|| self.output.clone())
            .flatten()
    }

    #[cfg(feature = "graph-analysis")]
    fn add_graph_inputs(&self, inputs: &mut Vec<ExternalInput>) -> Result<(), String> {
        if let Some(path) = self.repo_graph.as_deref() {
            inputs.push(path_input("repo_graph", path)?);
        }
        if let Some(path) = self.co_edit_journal.as_deref() {
            inputs.push(path_input("co_edit_journal", path)?);
        }
        Ok(())
    }
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            repo_root: PathBuf::from("."),
            profile: "gating".to_string(),
            format: "json".to_string(),
            include_references: false,
            method: AnalysisMethod::Auto,
            baseline: None,
            output: None,
            no_color: false,
            quiet: false,
            summary_only: false,
            paths: BTreeSet::new(),
            changed_ranges_json: None,
            #[cfg(feature = "graph-analysis")]
            repo_graph: None,
            #[cfg(feature = "graph-analysis")]
            co_edit_journal: None,
        }
    }
}

fn parse_changed_ranges(value: Option<&str>) -> Result<Vec<ChangedRange>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let parsed: serde_json::Value = serde_json::from_str(value)
        .map_err(|error| format!("invalid --changed-ranges-json: {error}"))?;
    let items = parsed
        .as_array()
        .ok_or_else(|| "--changed-ranges-json must be an array".to_string())?;
    items.iter().map(parse_changed_range).collect()
}

fn parse_changed_range(item: &serde_json::Value) -> Result<ChangedRange, String> {
    let object = item
        .as_object()
        .ok_or_else(|| "changed range must be an object".to_string())?;
    let path = object
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "changed range requires path".to_string())?;
    let start_line = range_line(object, "start_line")?;
    let end_line = range_line(object, "end_line")?;
    if start_line == 0 || end_line < start_line {
        return Err("changed range lines must satisfy 1 <= start <= end".to_string());
    }
    Ok(ChangedRange {
        path: path.to_string(),
        start_line,
        end_line,
    })
}

fn range_line(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<usize, String> {
    object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("changed range requires {key}"))
}

#[cfg(feature = "graph-analysis")]
fn path_input(kind: &str, path: &Path) -> Result<ExternalInput, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let schema_version = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value["schema_version"].as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    Ok(ExternalInput::from_text(
        ExternalInputMetadata::analysis(
            kind,
            path.display().to_string(),
            schema_version,
            "authoritative",
        ),
        &text,
    ))
}
