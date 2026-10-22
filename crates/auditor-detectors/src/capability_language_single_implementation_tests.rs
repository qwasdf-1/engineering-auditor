//! `single_implementation_abstraction` capability boundaries for each
//! language it reads. Positives declare a private abstraction with exactly
//! one implementation. Negatives hold the nearest forms that are not one: a
//! public abstraction, two implementations, one implementation and a test
//! fake, no implementation, a Rust blanket implementation and an extension
//! trait on a foreign type, a Java single-method interface and an extended
//! interface, a Python `Protocol`, a TypeScript interface, and a Go embedded
//! interface and build-constrained file.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "single_implementation_abstraction";

pub(crate) fn single_implementation_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector != DETECTOR {
        return None;
    }
    let files: Vec<(&str, &str)> = match (language, positive) {
        ("rust", true) => vec![("src/store.rs", RUST_SINGLE)],
        ("rust", false) => vec![("src/store.rs", RUST_VARIED), ("tests/clock.rs", RUST_FAKE)],
        ("python", true) => vec![("src/store.py", PYTHON_SINGLE)],
        ("python", false) => vec![
            ("src/store.py", PYTHON_VARIED),
            ("tests/test_clock.py", PYTHON_FAKE),
        ],
        ("typescript", true) => vec![("src/store.ts", TS_SINGLE)],
        ("typescript", false) => vec![("src/store.ts", TS_VARIED), ("src/clock.test.ts", TS_FAKE)],
        ("go", true) => vec![("store.go", GO_SINGLE)],
        ("go", false) => vec![
            ("store.go", GO_VARIED),
            ("store_test.go", GO_FAKE),
            ("poll_linux.go", GO_PLATFORM),
        ],
        ("java", true) => vec![("src/main/java/Store.java", JAVA_SINGLE)],
        ("java", false) => vec![
            ("src/main/java/Store.java", JAVA_VARIED),
            ("src/test/java/FakeClock.java", JAVA_FAKE),
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

const RUST_SINGLE: &str = r#"trait Store {
    fn get(&self, key: &str) -> Option<String>;
}

struct Disk;

impl Store for Disk {
    fn get(&self, key: &str) -> Option<String> {
        read(key)
    }
}

pub(crate) trait Codec {
    fn encode(&self, value: &Value) -> Vec<u8>;
}

struct Json;

impl Codec for Json {
    fn encode(&self, value: &Value) -> Vec<u8> {
        to_json(value)
    }
}
"#;

const RUST_VARIED: &str = r#"pub trait Store {
    fn get(&self, key: &str) -> Option<String>;
}

struct Disk;

impl Store for Disk {
    fn get(&self, key: &str) -> Option<String> {
        read(key)
    }
}

trait Codec {
    fn encode(&self) -> Vec<u8>;
}

struct Json;
struct Yaml;

impl Codec for Json {
    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }
}

impl Codec for Yaml {
    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }
}

trait Clock {
    fn now(&self) -> u64;
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        epoch()
    }
}

trait Describe {
    fn describe(&self) -> String;
}

impl<T: Debug> Describe for T {
    fn describe(&self) -> String {
        format!("{self:?}")
    }
}

trait Shout {
    fn shout(&self) -> String;
}

impl Shout for str {
    fn shout(&self) -> String {
        self.to_uppercase()
    }
}

trait Hook {
    fn run(&self);
}
"#;

const RUST_FAKE: &str = r#"struct FakeClock;

impl Clock for FakeClock {
    fn now(&self) -> u64 {
        0
    }
}
"#;

const PYTHON_SINGLE: &str = r#"from abc import ABC, abstractmethod


class _Store(ABC):
    @abstractmethod
    def get(self, key):
        ...


class _DiskStore(_Store):
    def get(self, key):
        return read(key)
"#;

const PYTHON_VARIED: &str = r#"from abc import ABC, ABCMeta, abstractmethod
from typing import Protocol


class Store(ABC):
    @abstractmethod
    def get(self, key):
        ...


class DiskStore(Store):
    def get(self, key):
        return read(key)


class _Codec(metaclass=ABCMeta):
    @abstractmethod
    def encode(self, value):
        ...


class _Json(_Codec):
    def encode(self, value):
        return dumps(value)


class _Yaml(_Codec):
    def encode(self, value):
        return dump(value)


class _Clock(ABC):
    @abstractmethod
    def now(self):
        ...


class _SystemClock(_Clock):
    def now(self):
        return time()


class _Reader(Protocol):
    def read(self):
        ...


class _FileReader(_Reader):
    def read(self):
        return ""


class _Hook(ABC):
    @abstractmethod
    def run(self):
        ...
"#;

const PYTHON_FAKE: &str = r#"from store import _Clock


class FakeClock(_Clock):
    def now(self):
        return 0
"#;

const TS_SINGLE: &str = r#"abstract class Store {
  abstract get(key: string): string | undefined;
}

class DiskStore extends Store {
  get(key: string) {
    return read(key);
  }
}
"#;

const TS_VARIED: &str = r#"export abstract class Store {
  abstract get(key: string): string | undefined;
}

class DiskStore extends Store {
  get(key: string) {
    return read(key);
  }
}

abstract class Codec {
  abstract encode(value: unknown): string;
}

class JsonCodec extends Codec {
  encode(value: unknown) {
    return JSON.stringify(value);
  }
}

class YamlCodec extends Codec {
  encode(value: unknown) {
    return dump(value);
  }
}

abstract class Clock {
  abstract now(): number;
}

class SystemClock extends Clock {
  now() {
    return Date.now();
  }
}

interface Reader {
  read(): string;
}

class FileReader implements Reader {
  read() {
    return "";
  }
}

abstract class Hook {
  abstract run(): void;
}
"#;

const TS_FAKE: &str = r#"class FakeClock extends Clock {
  now() {
    return 0;
  }
}
"#;

const GO_SINGLE: &str = r#"package store

type store interface {
	Get(key string) (string, bool)
	Put(key, value string)
}

type disk struct{}

func (d *disk) Get(key string) (string, bool) { return read(key) }

func (d *disk) Put(key, value string) { write(key, value) }
"#;

const GO_VARIED: &str = r#"package store

import "io"

type Store interface {
	Get(key string) (string, bool)
}

type disk struct{}

func (d *disk) Get(key string) (string, bool) { return read(key) }

type codec interface {
	Encode(value any) []byte
}

type jsonCodec struct{}

func (jsonCodec) Encode(value any) []byte { return marshal(value) }

type yamlCodec struct{}

func (yamlCodec) Encode(value any) []byte { return dump(value) }

type clock interface {
	Now() int64
}

type systemClock struct{}

func (systemClock) Now() int64 { return epoch() }

type readCloser interface {
	io.Reader
	Close() error
}

type file struct{}

func (file) Close() error { return nil }

type hook interface {
	Fire()
}
"#;

const GO_FAKE: &str = r#"package store

type fakeClock struct{}

func (fakeClock) Now() int64 { return 0 }
"#;

const GO_PLATFORM: &str = r#"//go:build linux

package store

type poller interface {
	Poll() int
}

type epoll struct{}

func (epoll) Poll() int { return 0 }
"#;

const JAVA_SINGLE: &str = r#"interface Store {
    String get(String key);

    void put(String key, String value);
}

final class DiskStore implements Store {
    public String get(String key) {
        return Disk.read(key);
    }

    public void put(String key, String value) {
        Disk.write(key, value);
    }
}

abstract class Codec {
    abstract byte[] encode(Object value);
}

final class JsonCodec extends Codec {
    byte[] encode(Object value) {
        return Json.write(value);
    }
}
"#;

const JAVA_VARIED: &str = r#"public interface Store {
    String get(String key);

    void put(String key, String value);
}

final class DiskStore implements Store {
    public String get(String key) {
        return Disk.read(key);
    }

    public void put(String key, String value) {
        Disk.write(key, value);
    }
}

interface Clock {
    long now();

    String zone();
}

final class SystemClock implements Clock {
    public long now() {
        return System.currentTimeMillis();
    }

    public String zone() {
        return "UTC";
    }
}

interface Handler {
    void handle(String event);
}

final class LogHandler implements Handler {
    public void handle(String event) {
        Log.info(event);
    }
}

interface Codec {
    byte[] encode(Object value);

    Object decode(byte[] bytes);
}

final class JsonCodec implements Codec {
    public byte[] encode(Object value) {
        return Json.write(value);
    }

    public Object decode(byte[] bytes) {
        return Json.read(bytes);
    }
}

final class YamlCodec implements Codec {
    public byte[] encode(Object value) {
        return Yaml.write(value);
    }

    public Object decode(byte[] bytes) {
        return Yaml.read(bytes);
    }
}

interface Base {
    void open();

    void close();
}

interface Channel extends Base {
    void send(String message);
}

final class Socket implements Channel {
    public void open() {}

    public void close() {}

    public void send(String message) {}
}

interface Hook {
    void before();

    void after();
}

abstract class BaseResource {
    static String header(String name) {
        return "X-" + name;
    }
}

final class FindingResource extends BaseResource {}

interface Worker {
    void start();

    void stop();
}

abstract class BaseWorker implements Worker {
    public void stop() {}
}

final class MailWorker extends BaseWorker {
    public void start() {}
}

final class SmsWorker extends BaseWorker {
    public void start() {}
}

public interface Registry {
    void register(String key);

    void unregister(String key);

    interface Entry {
        String key();

        String value();
    }

    static final class Pair implements Entry {
        public String key() {
            return "";
        }

        public String value() {
            return "";
        }
    }
}
"#;

const JAVA_FAKE: &str = r#"final class FakeClock implements Clock {
    public long now() {
        return 0;
    }

    public String zone() {
        return "test";
    }
}
"#;

/// Each positive reports exactly these abstractions, and its negative none.
#[test]
fn single_implementation_boundaries_report_exactly_the_private_abstractions() {
    let expected: &[(&str, &[&str])] = &[
        ("rust", &["Store", "Codec"]),
        ("python", &["_Store"]),
        ("typescript", &["Store"]),
        ("go", &["store"]),
        ("java", &["Store", "Codec"]),
    ];
    let mut failures = Vec::new();
    for (language, names) in expected {
        for positive in [true, false] {
            let files =
                single_implementation_capability_files(DETECTOR, language, positive).unwrap();
            let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let reported = analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == DETECTOR)
                .filter_map(|finding| finding.identity.get("abstraction").cloned())
                .collect::<BTreeSet<_>>();
            let wanted = if positive {
                names.iter().map(|name| name.to_string()).collect()
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
