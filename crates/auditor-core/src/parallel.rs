use std::sync::atomic::{AtomicUsize, Ordering};

/// Applies `f` to every item on all available cores and returns the results
/// in input order, so callers stay deterministic. Workers take the next
/// unclaimed item, which balances uneven per-item cost.
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(items.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut indexed: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break done;
                        };
                        done.push((index, f(item)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("parallel_map worker panicked"))
            .collect()
    });
    indexed.sort_unstable_by_key(|(index, _)| *index);
    indexed.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests {
    use super::parallel_map;

    #[test]
    fn results_keep_input_order() {
        let items: Vec<u64> = (0..1000).collect();
        let squares = parallel_map(&items, |value| value * value);
        assert_eq!(
            squares,
            items.iter().map(|value| value * value).collect::<Vec<_>>()
        );
        assert!(parallel_map(&[] as &[u64], |value| *value).is_empty());
    }
}
