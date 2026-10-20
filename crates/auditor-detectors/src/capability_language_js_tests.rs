//! JavaScript and TypeScript capability boundaries for the catalog
//! language-claim tests.

/// JS/TS boundaries for rules resolved through API identity. Each negative is
/// the nearest safe form of its positive, through the same module binding.
pub(super) fn js_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (imports, positive_body, negative_body): (&str, &str, &str) = match detector {
        "shell_execution" => (
            "const { exec, execFile } = require('node:child_process');",
            "exec(command);",
            "execFile('git', ['status', command]);",
        ),
        "exit_in_production" => ("", "process.exit(1);", "process.exitCode = 1;"),
        "todo_unimplemented_in_production" => (
            "",
            "throw new Error('not implemented');",
            "throw new Error('invariant violated');",
        ),
        "eval_in_production" => (
            "",
            "return eval(command);",
            "return JSON.parse(command);",
        ),
        "unsafe_deserialization" => (
            "const serialize = require('node-serialize');",
            "return serialize.unserialize(command);",
            "return JSON.parse(command);",
        ),
        "bind_all_interfaces" => (
            "import http from 'node:http';",
            "http.createServer().listen(8080, '0.0.0.0');",
            "http.createServer().listen(8080, '127.0.0.1');",
        ),
        "world_writable_file" => (
            "import fs from 'fs';",
            "fs.writeFileSync(command, '', { mode: 0o666 });",
            "fs.writeFileSync(command, '', { mode: 0o644 });",
        ),
        "random_for_security" => (
            "import { randomInt } from 'crypto';",
            "const token = Math.random();",
            "const token = randomInt(10);",
        ),
        "request_no_cert_verify" => (
            "import https from 'https';",
            "return new https.Agent({ rejectUnauthorized: false });",
            "return new https.Agent({ rejectUnauthorized: true });",
        ),
        "ssl_with_bad_defaults" => (
            "import tls from 'tls';",
            "return tls.connect({ host: command, minVersion: 'TLSv1' });",
            "return tls.connect({ host: command, minVersion: 'TLSv1.2' });",
        ),
        "ssh_no_host_key_verification" => (
            "import { Client } from 'ssh2';",
            "new Client().connect({ host: command, hostVerifier: () => true });",
            "new Client().connect({ host: command, hostVerifier: (key) => key === command });",
        ),
        "xml_xxe_vulnerable" => (
            "import libxmljs from 'libxmljs';",
            "return libxmljs.parseXml(command, { noent: true });",
            "return libxmljs.parseXml(command, { noent: false });",
        ),
        "insecure_cryptography" => (
            "import crypto from 'crypto';",
            "return crypto.createHash('md5').update(command).digest('hex');",
            "return crypto.createHash('sha256').update(command).digest('hex');",
        ),
        "resource_not_closed" => (
            "import fs from 'fs';",
            "const fd = fs.openSync(command, 'r');\n  fs.readSync(fd, Buffer.alloc(8));",
            "const fd = fs.openSync(command, 'r');\n  fs.readSync(fd, Buffer.alloc(8));\n  fs.closeSync(fd);",
        ),
        "assignment_in_operand" => (
            "",
            "let ready;\n  if (ready = command.trim()) consume(ready);",
            "let match;\n  while ((match = /a/g.exec(command)) !== null) consume(match);",
        ),
        "unused_assignment" => (
            "",
            "let mode = 'a';\n  mode = command.trim();\n  return mode;",
            "let mode = command;\n  consume(mode);\n  mode = command.trim();\n  return `${mode}`;",
        ),
        "sql_injection" => (
            "",
            "return db.query(`SELECT id FROM users WHERE name = '${command}'`);",
            "return db.query('SELECT id FROM users WHERE name = $1', [`${command}`]);",
        ),
        "hardcoded_credential" => (
            "",
            "const apiKey = 'tok_9f8e7d6c5b4a3210';\n  return apiKey;",
            "const apiKeyEnv = 'SERVICE_API_KEY';\n  return process.env[apiKeyEnv];",
        ),
        "hardcoded_tmp_directory" => (
            "import os from 'node:os';\nimport path from 'node:path';",
            "return path.join('/tmp/scratch', command);",
            "return path.join(os.tmpdir(), command);",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    let (extension, parameter) = match language {
        "typescript" => ("ts", "command: string"),
        _ => ("js", "command"),
    };
    Some(vec![(
        format!("src/capability.{extension}"),
        format!("{imports}\n\nexport function inspect({parameter}) {{\n  {body}\n}}\n"),
    )])
}

/// JS/TS boundaries for the L3 collection and L5 receiver facts. JavaScript
/// proves types through JSDoc (`@param {number[]} items`), TypeScript through
/// annotations; each negative is the nearest correct form of its positive.
pub(super) fn js_fact_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let source = match (language, detector, positive) {
        ("javascript", "repeated_sort_in_loop", true) => "/**\n * @param {Request[]} requests\n * @param {number[]} candidates\n */\nfunction inspect(requests, candidates) {\n  for (const request of requests) { candidates.sort(); consume(request, candidates[0]) }\n}\n",
        ("javascript", "repeated_sort_in_loop", false) => "/** @param {number[][]} groups */\nfunction inspect(groups) {\n  for (const group of groups) { const values = [...group]; values.sort(); consume(values) }\n}\n",
        ("javascript", "sort_for_extremum", true) => "/** @param {number[]} items */\nfunction smallest(items) { items.sort(); return items.at(0) }\n",
        ("javascript", "sort_for_extremum", false) => "/** @param {number[]} items */\nfunction ordered(items) { items.sort(compare); return items }\n",
        ("javascript", "full_sort_for_top_k", true) => "/** @param {number[]} items */\nfunction smallest(items) { items.sort(); return items.slice(0, 5) }\n",
        ("javascript", "full_sort_for_top_k", false) => "/** @param {number[]} items */\nfunction smallest(items) { items.sort(); return items.at(0) }\n",
        ("javascript", "repeated_collection_traversal", true) => "/** @param {Item[]} items */\nfunction inspect(items) {\n  consume(items.some(item => item.ready))\n  consume(items.every(item => item.valid))\n  consume(items.find(item => item.target))\n}\n",
        ("javascript", "repeated_collection_traversal", false) => "/**\n * @param {Item[]} ready\n * @param {Item[]} valid\n * @param {Item[]} targets\n */\nfunction distinct(ready, valid, targets) {\n  consume(ready.some(item => item.ready))\n  consume(valid.every(item => item.valid))\n  consume(targets.find(item => item.target))\n}\n/** @param {Item[]} items */\nfunction changed(items) {\n  consume(items.some(item => item.ready))\n  mutate(items)\n  consume(items.every(item => item.valid))\n  consume(items.find(item => item.target))\n}\n",
        ("javascript", "hot_path_linear_membership", true) => "/**\n * @param {string[]} ids\n * @param {string[]} wanted\n */\nfunction select(ids, wanted) {\n  for (const id of ids) { if (wanted.includes(id)) consume(id) }\n}\n",
        ("javascript", "hot_path_linear_membership", false) => "/**\n * @param {string[]} ids\n * @param {string} wanted\n */\nfunction select(ids, wanted) {\n  for (const id of ids) { if (wanted.includes(id)) consume(id) }\n}\n",
        ("javascript", "nested_cross_collection_traversal", true) => "/**\n * @param {Item[]} left\n * @param {Item[]} right\n */\nfunction compare(left, right) {\n  for (const outer of left) { for (const inner of right) { if (outer.id === inner.id) consume(outer, inner) } }\n}\n",
        ("javascript", "nested_cross_collection_traversal", false) => "/**\n * @param {Item[]} left\n * @param {Item[]} right\n */\nfunction visit(left, right) { for (const outer of left) consume(outer); for (const inner of right) consume(inner) }\n",
        ("javascript", "cache_miss_opportunity", true) => "/** @param {number} data */\nfunction inspect(data) { return Math.abs(data) === Math.abs(data) && Math.abs(data) > 0 }\n",
        ("javascript", "cache_miss_opportunity", false) => "/** @param {number} data */\nfunction inspect(data) { return Math.abs(data) === Math.abs(data) }\n",
        ("javascript", "loop_invariant_computation", true) => "/**\n * @param {number[]} items\n * @param {number} limit\n */\nfunction render(items, limit) { for (const item of items) consume(item, Math.abs(limit)) }\n",
        ("javascript", "loop_invariant_computation", false) => "/** @param {number[]} items */\nfunction render(items) { for (const item of items) consume(item, Math.abs(item)) }\n",
        ("javascript", "immutable_string_concatenation_in_loop", true) => "/** @param {string[]} items */\nfunction render(items) { let output = ''; for (const item of items) output += item; return output }\n",
        ("javascript", "immutable_string_concatenation_in_loop", false) => "/** @param {string[]} items */\nfunction render(items) { return items.join('') }\n",
        ("javascript", "intermediate_collection_materialization", true) => "/** @param {number[]} items */\nfunction anyPositive(items) { return items.filter(item => item > 0).length > 0 }\n",
        ("javascript", "intermediate_collection_materialization", false) => "/** @param {number[]} items */\nfunction anyPositive(items) { return items.some(item => item > 0) }\n/** @param {number[]} items */\nfunction logged(items) { return items.filter(item => check(item)).length > 0 }\n/** @param {string[]} chars */\nfunction spaces(chars) { const count = chars.filter(char => char === \" \").length; return count }\n",
        ("typescript", "intermediate_collection_materialization", true) => "function anyPositive(items: number[]) { return items.filter(item => item > 0).length > 0 }\n",
        ("typescript", "intermediate_collection_materialization", false) => "function anyPositive(items: number[]) { return items.some(item => item > 0) }\nfunction logged(items: number[]) { return items.filter(item => check(item)).length > 0 }\nfunction spaces(chars: string[]) { const count = chars.filter(char => char === \" \").length; return count }\n",
        ("javascript", "manual_collection_copy_loop", true) => "/** @param {number[]} source */\nfunction duplicate(source) {\n  const target = new Array(source.length);\n  for (let index = 0; index < source.length; index++) { target[index] = source[index] }\n  return target\n}\n",
        ("javascript", "manual_collection_copy_loop", false) => "/** @param {number[]} source */\nfunction duplicate(source) {\n  const target = new Array(source.length);\n  for (let index = 0; index < source.length; index++) { target[index] = source[index] * 2 }\n  return target\n}\n",
        ("typescript", "manual_collection_copy_loop", true) => "function duplicate(source: number[]): number[] {\n  const target: number[] = new Array(source.length);\n  for (let index = 0; index < source.length; index++) { target[index] = source[index] }\n  return target\n}\n",
        ("typescript", "manual_collection_copy_loop", false) => "function duplicate(source: number[]): number[] {\n  const target: number[] = new Array(source.length);\n  for (let index = 0; index < source.length; index++) { target[index] = source[index] * 2 }\n  return target\n}\n",
        ("javascript", "repeated_format_allocation", true) => "const util = require('node:util');\n/**\n * @param {number[]} items\n * @param {string} mode\n */\nfunction render(items, mode) { for (const item of items) consume(item, util.format('mode=%s', mode)) }\n",
        ("javascript", "repeated_format_allocation", false) => "const util = require('node:util');\n/** @param {number[]} items */\nfunction render(items) { for (const item of items) consume(util.format('%d', item)) }\n",
        ("typescript", "repeated_format_allocation", true) => "import { format } from 'node:util';\nfunction render(items: number[], mode: string) { for (const item of items) consume(item, format('mode=%s', mode)) }\n",
        ("typescript", "repeated_format_allocation", false) => "import { format } from 'node:util';\nfunction render(items: number[]) { for (const item of items) consume(format('%d', item)) }\n",
        _ => return None,
    };
    let extension = if language == "typescript" { "ts" } else { "js" };
    Some(vec![(
        format!("src/capability.{extension}"),
        source.to_string(),
    )])
}
