//! `unbounded_concurrency` and `unbounded_channel_or_queue` capability
//! boundaries. Concurrency positives start a task per item of a collection;
//! its negatives hold the bounded forms: a literal collection, a counted
//! worker loop, a set of workers, and a semaphore or limiter. Queue
//! positives create channels and queues with no capacity; its negatives give
//! one. Test code holds the positives and is out of scope.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const CONCURRENCY: &str = "unbounded_concurrency";
const QUEUE: &str = "unbounded_channel_or_queue";

pub(crate) fn unbounded_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (path, test_path, unbounded, bounded) = match (detector, language) {
        (CONCURRENCY, "python") => ("src/fan.py", "tests/test_fan.py", PY_FAN, PY_BOUNDED_FAN),
        (CONCURRENCY, "javascript") => ("src/fan.js", "src/fan.test.js", JS_FAN, JS_BOUNDED_FAN),
        (CONCURRENCY, "typescript") => ("src/fan.ts", "src/fan.test.ts", JS_FAN, JS_BOUNDED_FAN),
        (CONCURRENCY, "go") => ("fan.go", "fan_test.go", GO_FAN, GO_BOUNDED_FAN),
        (CONCURRENCY, "rust") => ("src/fan.rs", "tests/fan.rs", RUST_FAN, RUST_BOUNDED_FAN),
        (CONCURRENCY, "java") => (
            "src/main/java/Fan.java",
            "src/test/java/FanTest.java",
            JAVA_FAN,
            JAVA_BOUNDED_FAN,
        ),
        (QUEUE, "python") => (
            "src/jobs.py",
            "tests/test_jobs.py",
            PY_QUEUE,
            PY_BOUNDED_QUEUE,
        ),
        (QUEUE, "rust") => (
            "src/jobs.rs",
            "tests/jobs.rs",
            RUST_QUEUE,
            RUST_BOUNDED_QUEUE,
        ),
        (QUEUE, "java") => (
            "src/main/java/Jobs.java",
            "src/test/java/JobsTest.java",
            JAVA_QUEUE,
            JAVA_BOUNDED_QUEUE,
        ),
        _ => return None,
    };
    let files = if positive {
        vec![(path, unbounded)]
    } else {
        vec![(path, bounded), (test_path, unbounded)]
    };
    Some(
        files
            .into_iter()
            .map(|(path, source)| (path.to_string(), source.to_string()))
            .collect(),
    )
}

const PY_FAN: &str = r#"import asyncio
import threading


async def fetch_all(urls):
    await asyncio.gather(*[fetch(url) for url in urls])


async def spawn_all(urls):
    for url in urls:
        asyncio.create_task(fetch(url))
    async with asyncio.TaskGroup() as group:
        for url in urls:
            group.create_task(fetch(url))


def thread_all(items):
    for item in items:
        threading.Thread(target=work, args=(item,)).start()
"#;

const PY_BOUNDED_FAN: &str = r#"import asyncio
import threading


async def fetch_pair():
    await asyncio.gather(*[fetch(url) for url in ["a", "b"]])


async def start_workers(queue):
    for _ in range(4):
        asyncio.create_task(worker(queue))


def run_workers(workers):
    for worker in workers:
        threading.Thread(target=worker).start()


async def fetch_limited(urls):
    semaphore = asyncio.Semaphore(8)
    await asyncio.gather(*[guarded(semaphore, url) for url in urls])
"#;

const JS_FAN: &str = r#"async function fetchAll(urls) {
  return Promise.all(urls.map((url) => fetch(url)));
}

async function settleAll(urls) {
  return Promise.allSettled(urls.map(fetch));
}
"#;

const JS_BOUNDED_FAN: &str = r#"async function fetchPair() {
  return Promise.all([first(), second()].map((promise) => promise));
}

async function fetchLimited(urls) {
  const limit = pLimit(8);
  return Promise.all(urls.map((url) => limit(() => fetch(url))));
}
"#;

const GO_FAN: &str = r#"package fan

func FetchAll(urls []string) {
	for _, url := range urls {
		go fetch(url)
	}
}
"#;

const GO_BOUNDED_FAN: &str = r#"package fan

func StartWorkers(jobs chan string) {
	for i := 0; i < 4; i++ {
		go worker(jobs)
	}
}

func RunWorkers(workers []Worker) {
	for _, worker := range workers {
		go worker.Run()
	}
}

func FetchLimited(urls []string) {
	slots := make(chan struct{}, 8)
	for _, url := range urls {
		slots <- struct{}{}
		go func(url string) {
			defer func() { <-slots }()
			fetch(url)
		}(url)
	}
}
"#;

const RUST_FAN: &str = r#"use std::thread;

pub async fn fetch_all(urls: Vec<String>) {
    for url in urls.clone() {
        tokio::spawn(fetch(url));
    }
    futures::future::join_all(urls.iter().map(|url| fetch(url.clone()))).await;
}

pub fn thread_all(items: Vec<Item>) {
    for item in items {
        thread::spawn(move || work(item));
    }
}
"#;

const RUST_BOUNDED_FAN: &str = r#"use std::sync::Arc;
use std::thread;

pub fn start_workers() {
    for index in 0..4 {
        thread::spawn(move || work(index));
    }
}

pub async fn fetch_limited(urls: Vec<String>) {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
    for url in urls {
        let permit = semaphore.clone().acquire_owned().await;
        tokio::spawn(async move {
            fetch(url).await;
            drop(permit);
        });
    }
}
"#;

const JAVA_FAN: &str = r#"import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

class Fan {
    void fetchAll(List<String> urls) {
        for (String url : urls) {
            new Thread(() -> fetch(url)).start();
        }
    }

    ExecutorService pool() {
        return Executors.newCachedThreadPool();
    }
}
"#;

const JAVA_BOUNDED_FAN: &str = r#"import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

class Fan {
    void startWorkers(List<Runnable> workers) {
        for (Runnable worker : workers) {
            new Thread(worker).start();
        }
    }

    void fetchAll(List<String> urls, ExecutorService executor) {
        for (String url : urls) {
            executor.submit(() -> fetch(url));
        }
    }
}
"#;

const PY_QUEUE: &str = r#"import asyncio
import queue
from multiprocessing import Queue

jobs = queue.Queue()
results = queue.Queue(maxsize=0)
events = queue.SimpleQueue()
tasks = asyncio.Queue()
shared = Queue()
"#;

const PY_BOUNDED_QUEUE: &str = r#"import asyncio
import queue


def make(limit, **options):
    jobs = queue.Queue(maxsize=100)
    results = queue.Queue(64)
    tasks = asyncio.Queue(limit)
    other = queue.Queue(**options)
    return jobs, results, tasks, other
"#;

const RUST_QUEUE: &str = r#"use std::sync::mpsc;

pub fn channels() {
    let (sender, receiver) = mpsc::channel::<Job>();
    let (events, inbox) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let (work, queue) = crossbeam_channel::unbounded::<Job>();
}
"#;

const RUST_BOUNDED_QUEUE: &str = r#"use std::sync::mpsc;

pub fn channels() {
    let (sender, receiver) = mpsc::sync_channel::<Job>(64);
    let (events, inbox) = tokio::sync::mpsc::channel::<Event>(64);
    let (work, queue) = crossbeam_channel::bounded::<Job>(64);
}

#[cfg(test)]
mod tests {
    #[test]
    fn sends() {
        let (sender, receiver) = std::sync::mpsc::channel::<u8>();
    }
}
"#;

const JAVA_QUEUE: &str = r#"import java.util.concurrent.BlockingQueue;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.PriorityBlockingQueue;

class Jobs {
    BlockingQueue<Runnable> jobs = new LinkedBlockingQueue<>();
    BlockingQueue<Runnable> ranked = new PriorityBlockingQueue<>();
    ExecutorService pool = Executors.newFixedThreadPool(4);
    ExecutorService single = Executors.newSingleThreadExecutor();
}
"#;

const JAVA_BOUNDED_QUEUE: &str = r#"import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;

class Jobs {
    BlockingQueue<Runnable> jobs = new LinkedBlockingQueue<>(100);
    BlockingQueue<Runnable> fixed = new ArrayBlockingQueue<>(100);
}
"#;

#[test]
fn unbounded_boundaries_report_exactly_the_unbounded_sites() {
    let py_fan = [
        "asyncio.gather(*[fetch(url) for url in urls])",
        "asyncio.create_task(fetch(url))",
        "group.create_task(fetch(url))",
        "threading.Thread(target=work, args=(item,))",
    ];
    let js_fan = [
        "Promise.all(urls.map((url) => fetch(url)))",
        "Promise.allSettled(urls.map(fetch))",
    ];
    let expected: &[(&str, &str, &[&str])] = &[
        (CONCURRENCY, "python", &py_fan),
        (CONCURRENCY, "javascript", &js_fan),
        (CONCURRENCY, "typescript", &js_fan),
        (CONCURRENCY, "go", &["go fetch(url)"]),
        (
            CONCURRENCY,
            "rust",
            &[
                "tokio::spawn(fetch(url))",
                "futures::future::join_all(urls.iter().map(|url| fetch(url.clone())))",
                "thread::spawn(move || work(item))",
            ],
        ),
        (
            CONCURRENCY,
            "java",
            &[
                "new Thread(() -> fetch(url))",
                "Executors.newCachedThreadPool()",
            ],
        ),
        (
            QUEUE,
            "python",
            &[
                "jobs = queue.Queue()",
                "results = queue.Queue(maxsize=0)",
                "events = queue.SimpleQueue()",
                "tasks = asyncio.Queue()",
                "shared = Queue()",
            ],
        ),
        (
            QUEUE,
            "rust",
            &[
                "let (sender, receiver) = mpsc::channel::<Job>();",
                "let (events, inbox) = tokio::sync::mpsc::unbounded_channel::<Event>();",
                "let (work, queue) = crossbeam_channel::unbounded::<Job>();",
            ],
        ),
        (
            QUEUE,
            "java",
            &[
                "jobs = new LinkedBlockingQueue<>()",
                "ranked = new PriorityBlockingQueue<>()",
                "pool = Executors.newFixedThreadPool(4)",
                "single = Executors.newSingleThreadExecutor()",
            ],
        ),
    ];
    let mut failures = Vec::new();
    for (detector, language, sites) in expected {
        for positive in [true, false] {
            let files = unbounded_capability_files(detector, language, positive).unwrap();
            let mut case = CapabilityCase::new(detector, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let reported = analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == *detector)
                .filter_map(|finding| finding.identity.get("site").cloned())
                .collect::<BTreeSet<_>>();
            let wanted = if positive {
                sites.iter().map(|site| site.to_string()).collect()
            } else {
                BTreeSet::new()
            };
            if reported != wanted {
                failures.push(format!(
                    "{detector} {language} {}: reported {reported:?}, expected {wanted:?}",
                    if positive { "positive" } else { "negative" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
