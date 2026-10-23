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

#[test]
fn select_literals_start_the_first_argument() {
    for arguments in [
        "\"SELECT * FROM posts WHERE uid=?\", (user.id,)",
        "'select name from t where id = %s', [row]",
        "f\"SELECT * FROM t WHERE id = {item}\"",
        "r\"\"\"\n    SELECT id FROM t WHERE k = ?\"\"\", (key,)",
        "`SELECT * FROM t WHERE id = $1`, id",
        "@\"SELECT * FROM t\", conn",
        "\"SELECT\\n*\", x",
    ] {
        assert!(super::starts_with_select_literal(arguments), "{arguments}");
    }
    for arguments in [
        "\"INSERT INTO t VALUES (?)\", (item,)",
        "\"UPDATE t SET a = ?\", (item,)",
        "query, (item,)",
        "build(\"SELECT * FROM t\")",
        "\"SELECTED_ROWS\"",
        "selector",
        "\"\"",
    ] {
        assert!(!super::starts_with_select_literal(arguments), "{arguments}");
    }
}

#[test]
fn python_db_api_selects_are_queries_whatever_the_receiver() {
    let source = "\
def posts(cur, users):
    out = []
    for user in users:
        cur.execute(\"SELECT * FROM posts WHERE uid = ?\", (user.id,))
        out.append(cur.fetchall())
    return out


def chained(conn, users):
    res = []
    for u in users:
        res.append(conn.execute(\"SELECT * FROM posts WHERE uid=?\", (u.id,)).fetchall())
    return res


def through_self(self, users):
    for user in users:
        consume(self.db.query(user.id))


def writes(cur, items):
    for item in items:
        cur.execute(\"INSERT INTO t VALUES (?)\", (item,))


def batched(cur, items):
    for item in items:
        prepare(item)
    cur.executemany(\"SELECT * FROM t WHERE id = ?\", [(item,) for item in items])


def invariant(cur, items):
    for item in items:
        consume(item, cur.execute(\"SELECT count(*) FROM t\"))


def not_stores(feedback, dbg, items):
    for item in items:
        consume(feedback.query(item))
        consume(dbg.query(item))
";
    assert_eq!(
        messages("python-db-api", "app/posts.py", source),
        [
            "Function `posts` calls `cur.execute` with loop-derived `user`.",
            "Function `chained` calls `conn.execute` with loop-derived `u`.",
            "Function `through_self` calls `db.query` with loop-derived `user`.",
        ]
    );
}

#[test]
fn db_receivers_and_jdbc_selects_in_other_languages() {
    let javascript = "\
async function rows(db, ids) {
    const out = [];
    for (const id of ids) {
        out.push(await db.query(\"SELECT * FROM t WHERE id = $1\", [id]));
    }
    return out;
}

async function userDbRows(userDb, ids) {
    for (const id of ids) {
        consume(await userDb.query(\"SELECT * FROM t WHERE id = $1\", [id]));
    }
}
";
    let java = "\
package app;

import java.sql.*;
import java.util.*;

class Lookup {
    void names(Statement stmt, List<String> ids) throws SQLException {
        for (String id : ids) {
            consume(stmt.executeQuery(\"SELECT name FROM t WHERE id = '\" + id + \"'\"));
        }
    }

    void updates(Statement stmt, List<String> ids) throws SQLException {
        for (String id : ids) {
            consume(stmt.executeQuery(\"UPDATE t SET seen = 1 WHERE id = '\" + id + \"'\"));
        }
    }
}
";
    assert_eq!(
        messages("js-db", "server/rows.js", javascript),
        [
            "Function `rows` calls `db.query` with loop-derived `id`.",
            "Function `userDbRows` calls `userDb.query` with loop-derived `id`.",
        ]
    );
    assert_eq!(
        messages("jdbc", "src/main/java/app/Lookup.java", java),
        ["Function `names` calls `stmt.executeQuery` with loop-derived `id`."]
    );
}

#[test]
fn helpers_pass_select_runners_to_their_callers() {
    let source = "\
def load(cur, uid):
    cur.execute(\"SELECT * FROM posts WHERE uid = ?\", (uid,))
    return cur.fetchall()


def each(cur, users):
    for user in users:
        consume(load(cur, user.id))
";
    assert_eq!(
        messages("python-helper", "app/load.py", source),
        ["Function `each` calls `cur.execute` with loop-derived `user`, through `load`."]
    );
}

#[test]
fn python_comprehensions_repeat_their_element_per_item() {
    let source = "\
def listed(users, conn):
    return [conn.execute(\"SELECT * FROM t WHERE id=?\", (u.id,)) for u in users]


def keyed(users, db):
    return {u.id: db.execute(\"SELECT * FROM t WHERE id=?\", (u.id,)).fetchone() for u in users}


def nested(groups, conn):
    return [conn.execute(\"SELECT * FROM t WHERE g=?\", (g,)) for gs in groups for g in gs]


def iterable_runs_once(conn, uid):
    return [row for row in conn.execute(\"SELECT * FROM t WHERE id=?\", (uid,))]


def walks_chunks(conn, ids):
    return [conn.execute(\"SELECT * FROM t WHERE id IN ?\", (chunk,)) for chunk in chunked(ids, 100)]


def lookup(conn, uid):
    return conn.execute(\"SELECT * FROM t WHERE id=?\", (uid,)).fetchone()


def through_helper(users, conn):
    return [lookup(conn, u.id) for u in users]
";
    assert_eq!(
        messages("python-comprehensions", "app/rows.py", source),
        [
            "Function `listed` calls `conn.execute` with loop-derived `u`.",
            "Function `keyed` calls `db.execute` with loop-derived `u`.",
            "Function `nested` calls `conn.execute` with loop-derived `g`.",
            "Function `through_helper` calls `conn.execute` with loop-derived `u`, through `lookup`.",
        ]
    );
}

#[test]
fn ado_net_command_reads_bind_their_parameters() {
    let source = "\
using System.Data.SqlClient;

class Repo {
    void Constructed(SqlConnection c, int[] ids) {
        foreach (var id in ids) {
            using var cmd = new SqlCommand(\"SELECT * FROM t WHERE id=@id\", c);
            cmd.Parameters.AddWithValue(\"@id\", id);
            using var reader = cmd.ExecuteReader();
        }
    }

    void Prepared(SqlConnection c, int[] ids) {
        var cmd = c.CreateCommand();
        cmd.CommandText = \"SELECT name FROM t WHERE id=@id\";
        foreach (var id in ids) {
            cmd.Parameters[\"@id\"].Value = id;
            cmd.ExecuteScalar();
        }
    }

    void Deletes(SqlConnection c, int[] ids) {
        foreach (var id in ids) {
            var cmd = new SqlCommand(\"DELETE FROM t WHERE id=@id\", c);
            cmd.Parameters.AddWithValue(\"@id\", id);
            cmd.ExecuteReader();
        }
    }

    void Invariant(SqlConnection c, int[] ids) {
        foreach (var id in ids) {
            var cmd = new SqlCommand(\"SELECT count(*) FROM t\", c);
            Consume(id, cmd.ExecuteScalar());
        }
    }
}
";
    assert_eq!(
        messages("ado-net", "src/Repo.cs", source),
        [
            "Function `Constructed` calls `cmd.ExecuteReader` with loop-derived `id`.",
            "Function `Prepared` calls `cmd.ExecuteScalar` with loop-derived `id`.",
        ]
    );
}

#[test]
fn utility_classes_named_for_a_store_are_not_stores() {
    let source = "class A {\n    void load(List<String> ids, Repository repo) {\n        for (String id : ids) {\n            DBUtils.findObjectById(id);\n            DbHelper.getRowById(id);\n            repo.findById(id);\n        }\n    }\n}\n";
    let found = messages("utility", "src/main/java/A.java", source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("findById"), "{found:?}");
}
