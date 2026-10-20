use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{analyze_repository, ScanProfile, SourceDiscoveryOptions};

/// The `n_plus_one_query_in_loop` messages for one source file.
fn messages(name: &str, relative: &str, source: &str) -> Vec<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root: PathBuf =
        std::env::temp_dir().join(format!("ea-queries-{name}-{}-{nanos}", std::process::id()));
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, source).unwrap();
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let _ = std::fs::remove_dir_all(root);
    analysis
        .findings
        .into_iter()
        .filter(|finding| finding.detector == "n_plus_one_query_in_loop")
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn point_lookups_read_one_record_by_key() {
    for method in [
        "findById",
        "FindByID",
        "getObjectByUuid",
        "load_by_key",
        "getVulnerabilityByVulnId",
    ] {
        assert!(super::is_read_query(method), "{method}");
    }
    for method in [
        "get",
        "by_id",
        "getById2",
        "findByName",
        "getOrCreateById",
        "byId",
    ] {
        assert!(!super::is_read_query(method), "{method}");
    }
}

#[test]
fn java_query_managers_are_stores_as_fields_parameters_and_resources() {
    let source = "\
package app;

import java.util.*;

class Exporter {
    private final QueryManager qm;
    private final Map<String, Item> cache = new HashMap<>();

    Exporter(QueryManager qm) {
        this.qm = qm;
    }

    void fields(List<Finding> findings, String fixed) {
        for (Finding finding : findings) {
            consume(qm.getObjectByUuid(Component.class, finding.uuid()));
            consume(qm.getObjectByUuid(Component.class, fixed));
            consume(cache.getObjectByUuid(finding.uuid()));
        }
    }

    void parameters(QueryManager manager, List<Team> teams) {
        for (Team team : teams) {
            consume(manager.getTeamByKey(team.key()));
        }
    }

    void resources(List<String> ids) {
        try (QueryManager scoped = new QueryManager()) {
            for (String id : ids) {
                consume(scoped.findProjectById(id));
            }
        }
        try (final var inferred = new QueryManager(request())) {
            for (String id : ids) {
                consume(inferred.findProjectById(id));
            }
        }
    }
}
";
    assert_eq!(
        messages("java-stores", "src/main/java/app/Exporter.java", source),
        [
            "Function `fields` calls `qm.getObjectByUuid` with loop-derived `finding`.",
            "Function `parameters` calls `manager.getTeamByKey` with loop-derived `team`.",
            "Function `resources` calls `scoped.findProjectById` with loop-derived `id`.",
            "Function `resources` calls `inferred.findProjectById` with loop-derived `id`.",
        ]
    );
}

#[test]
fn javascript_store_modules_bind_query_interfaces() {
    let redbean = "\
const { R } = require(\"redbean-node\");

async function heartbeats(monitors) {
    for (const monitor of monitors) {
        consume(await R.findOne(\"heartbeat\", \"monitor_id = ?\", [monitor.id]));
    }
}
";
    let ramda = "\
const R = require(\"ramda\");

function pick(rows) {
    for (const row of rows) {
        consume(R.findOne(row));
    }
}
";
    assert_eq!(
        messages("redbean", "server/status.js", redbean),
        ["Function `heartbeats` calls `R.findOne` with loop-derived `monitor`."]
    );
    assert_eq!(
        messages("ramda", "server/pick.js", ramda),
        Vec::<String>::new()
    );
}
