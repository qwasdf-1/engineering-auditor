//! `unbounded_channel_or_queue` bounds the code enforces itself: a Rust
//! channel of `()` wake-ups, a Python or Java queue whose producers compare
//! its size against a limit, and a Java executor shut down in the method that
//! creates it. Each shape is paired with the near miss that stays reported.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

/// The `site` identity of every finding in a single-file repository.
fn reported_sites(path: &str, source: &str) -> BTreeSet<String> {
    let case = CapabilityCase::new(
        "unbounded_channel_or_queue",
        ScanProfile::Full,
        path,
        source,
        path,
        source,
    );
    analyze_case(&case, true, ScanProfile::Full)
        .findings
        .iter()
        .filter(|finding| finding.detector == "unbounded_channel_or_queue")
        .filter_map(|finding| finding.identity.get("site").cloned())
        .collect()
}

fn assert_reported(path: &str, source: &str, expected: &[&str]) {
    let wanted = expected.iter().map(|site| site.to_string()).collect();
    assert_eq!(reported_sites(path, source), wanted);
}

const RUST_SIGNALS: &str = r#"use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub struct Handle {
    abort_tx: mpsc::UnboundedSender<()>,
}

impl Handle {
    pub fn new() -> (Self, UnboundedReceiver<()>) {
        let (abort_tx, abort_rx) = mpsc::unbounded_channel();
        (Self { abort_tx }, abort_rx)
    }
}

pub struct Client {
    wake_rx: UnboundedReceiver<()>,
}

pub struct Handles {
    pub cancel: UnboundedSender<()>,
}

pub fn connect() -> Handles {
    let (cancel_tx, wake_rx) = mpsc::unbounded_channel();
    let client = Client { wake_rx };
    let (stop_tx, _stop_rx) = mpsc::unbounded_channel();
    Handles { cancel: stop_tx }
}

pub fn turbofish() {
    let (tx, rx) = mpsc::unbounded_channel::<()>();
    let (std_tx, std_rx) = std::sync::mpsc::channel::<()>();
}

pub fn annotated() {
    let (tx, rx): (UnboundedSender<()>, UnboundedReceiver<()>) = mpsc::unbounded_channel();
}
"#;

const RUST_DATA: &str = r#"use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub struct Sink {
    tx: UnboundedSender<Message>,
}

impl Sink {
    pub fn new() -> (Self, UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

pub struct Other {
    wake: UnboundedSender<()>,
}

pub fn events() {
    let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
    let (bare_tx, bare_rx) = mpsc::unbounded_channel();
    let (named_tx, named_rx) = mpsc::unbounded_channel();
    let other = Other { wake: wake_tx };
    let (job_tx, job_rx): (UnboundedSender<Job>, UnboundedReceiver<Job>) = mpsc::unbounded_channel();
}
"#;

#[test]
fn rust_unit_signal_channels_are_not_reported() {
    assert_reported("src/signals.rs", RUST_SIGNALS, &[]);
}

#[test]
fn rust_data_channels_stay_reported() {
    assert_reported(
        "src/data.rs",
        RUST_DATA,
        &[
            "let (tx, rx) = mpsc::unbounded_channel();",
            "let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();",
            "let (bare_tx, bare_rx) = mpsc::unbounded_channel();",
            "let (named_tx, named_rx) = mpsc::unbounded_channel();",
            "let (job_tx, job_rx): (UnboundedSender<Job>, UnboundedReceiver<Job>) = mpsc::unb",
        ],
    );
}

const PY_CAPPED: &str = r#"import asyncio
import queue


class Manager:
    def __init__(self):
        self.forward = asyncio.PriorityQueue()
        self.replies = queue.Queue()
        self.gate = queue.Queue()
        self.counted = queue.Queue()

    def add(self, item):
        if self.forward.qsize() >= self.MAX:
            return
        self.forward.put_nowait(item)

    def reply(self, item):
        while self.replies.qsize() > 10:
            pass
        self.replies.put(item)

    def push(self, item):
        if self.MAX < self.gate.qsize():
            return
        self.gate.put(item)

    def count(self, item):
        if len(self.counted) <= self.MAX:
            self.counted.put(item)


def local_cap(item):
    pending = queue.Queue()
    if pending.qsize() < LIMIT:
        pending.put(item)
"#;

const PY_UNCAPPED: &str = r#"import queue


class Manager:
    def __init__(self):
        self.logged = queue.Queue()
        self.empty_checked = queue.Queue()
        self.other = queue.Queue()

    def run(self, item):
        log.info(self.logged.qsize())
        if self.empty_checked.qsize() == 0:
            return
        if self.empty_checked.qsize() > 0:
            return
        if self.unrelated.qsize() >= LIMIT:
            return
"#;

#[test]
fn python_size_checked_queues_are_not_reported() {
    assert_reported("src/capped.py", PY_CAPPED, &[]);
}

#[test]
fn python_queues_without_a_size_cap_stay_reported() {
    assert_reported(
        "src/uncapped.py",
        PY_UNCAPPED,
        &[
            "self.logged = queue.Queue()",
            "self.empty_checked = queue.Queue()",
            "self.other = queue.Queue()",
        ],
    );
}

const JAVA_CAPPED: &str = r#"import java.util.concurrent.LinkedBlockingQueue;

class Audit {
    LinkedBlockingQueue<Event> queue = new LinkedBlockingQueue<Event>();
    private final LinkedBlockingQueue<Event> held;

    Audit() {
        this.held = new LinkedBlockingQueue<>();
    }

    void log(Event event) {
        if (queue.size() >= getMaxQueueSize()) {
            return;
        }
        queue.add(event);
    }

    void hold(Event event) {
        if (this.held.size() < max) {
            held.add(event);
        }
    }
}
"#;

const JAVA_UNCAPPED: &str = r#"import java.util.concurrent.LinkedBlockingQueue;

class Audit {
    LinkedBlockingQueue<Event> logged = new LinkedBlockingQueue<Event>();
    LinkedBlockingQueue<Event> metered = new LinkedBlockingQueue<Event>();
    LinkedBlockingQueue<Event> checked = new LinkedBlockingQueue<Event>();
    LinkedBlockingQueue<Event> mine = new LinkedBlockingQueue<Event>();

    void run() {
        logger.info("length={}", logged.size());
        metrics.gauge(metered.size());
        if (checked.size() > 0 || checked.isEmpty()) {
            return;
        }
        if (theirs.size() >= max) {
            return;
        }
    }
}
"#;

#[test]
fn java_size_checked_queues_are_not_reported() {
    assert_reported("src/main/java/Audit.java", JAVA_CAPPED, &[]);
}

#[test]
fn java_queues_without_a_size_cap_stay_reported() {
    assert_reported(
        "src/main/java/Audit.java",
        JAVA_UNCAPPED,
        &[
            "logged = new LinkedBlockingQueue<Event>()",
            "metered = new LinkedBlockingQueue<Event>()",
            "checked = new LinkedBlockingQueue<Event>()",
            "mine = new LinkedBlockingQueue<Event>()",
        ],
    );
}

const JAVA_SCOPED: &str = r#"import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

class Auth {
    String token() {
        ExecutorService service = null;
        try {
            service = Executors.newFixedThreadPool(1);
            return acquire(service);
        } finally {
            if (service != null) {
                service.shutdown();
            }
        }
    }

    String direct() {
        ExecutorService single = Executors.newSingleThreadExecutor();
        try {
            return acquire(single);
        } finally {
            single.shutdownNow();
        }
    }

    String resource() throws Exception {
        try (ExecutorService pool = Executors.newFixedThreadPool(2)) {
            return acquire(pool);
        }
    }
}
"#;

const JAVA_UNSCOPED: &str = r#"import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

class Auth {
    private final ExecutorService stored = Executors.newFixedThreadPool(4);
    private ExecutorService late;

    void init() {
        late = Executors.newSingleThreadExecutor();
        stored.shutdown();
    }

    String leaked() {
        ExecutorService local = Executors.newFixedThreadPool(1);
        return acquire(local);
    }

    String other() {
        ExecutorService mine = Executors.newFixedThreadPool(1);
        theirs.shutdown();
        return acquire(mine);
    }
}
"#;

#[test]
fn java_executors_shut_down_in_their_method_are_not_reported() {
    assert_reported("src/main/java/Auth.java", JAVA_SCOPED, &[]);
}

#[test]
fn java_executors_that_outlive_the_method_stay_reported() {
    assert_reported(
        "src/main/java/Auth.java",
        JAVA_UNSCOPED,
        &[
            "stored = Executors.newFixedThreadPool(4)",
            "late = Executors.newSingleThreadExecutor()",
            "local = Executors.newFixedThreadPool(1)",
            "mine = Executors.newFixedThreadPool(1)",
        ],
    );
}
