use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub trait CacheEnabled {
    type CacheType;
    type KeyType;
    type EntryType;

    fn make_cache(max_memory_cost: usize) -> Self::CacheType;
}

struct Entry<V> {
    computation_cost: usize,
    memory_cost: usize,
    cost_ratio: f64,
    last_use_gen: AtomicUsize,
    value: Arc<V>,
}

/// This cache works in two stages - queryable stage, and update stage.
///
/// During the queryable stage the state of the cache is frozen. New computed values
/// are queued for inclusion in the cache.
/// IMPORTANT: This queue is unbounded and is not considered when calculating
///            the total memory usage. The user must ensure that there is enough memory
///            for whatever they are doing during the queryable stage.
///            DO NOT FORGET to call `update()` whenever you want this queue to be processed.
///
/// In the update stage the queue of new cached values is consolidated to the
/// cache entries, their memory cost tallied, and if memory cost is determined
/// to be higher than the allowed limit the cleanup procedure is engaged.
///
/// During cleanup a heuristic is used to drop the least useful entries.
/// Entries are dropped until the total memory cost drops below the allowed limit.
/// NOTE: There is some leeway in how many entries are dropped. In particular more
///       entries may be dropped than necessary to reduce the frequency of cleanups.
pub struct LockStepCache<K, V> {
    entries: BTreeMap<K, Entry<V>>,
    // Only accounts for the total memory cost of `entries`. The memory cost of
    // `new_entries` is added when they are moved to `entries`.
    // That way it doesn't have to be atomic, and we can handle duplicated keys
    // in `new_entries` just in case.
    total_memory_cost: usize,
    // VecDeque instead of Vec because it has smaller worst-case pause on reallocation.
    new_entries: Mutex<VecDeque<(K, Entry<V>)>>,
    new_memory_cost: usize,
    curr_gen: usize,
    max_memory_cost: usize,
}

impl<K, V> LockStepCache<K, V> {
    pub fn new(max_memory_cost: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            total_memory_cost: 0,
            new_entries: Mutex::new(VecDeque::new()),
            new_memory_cost: 0,
            curr_gen: 0,
            max_memory_cost,
        }
    }

    pub fn set_max_memory_cost(&mut self, max_memory_cost: usize) {
        self.max_memory_cost = max_memory_cost;
    }

    pub fn invalidate_all(&mut self) {
        self.entries.clear();
        self.total_memory_cost = 0;
        self.new_entries.lock().unwrap().clear();
        self.new_memory_cost = 0;
    }
}

impl<K, V> LockStepCache<K, V>
where
    K: Ord + Clone,
{
    pub fn try_get(&self, key: &K) -> Option<Arc<V>> {
        self.entries.get(key).map(|entry| {
            entry.last_use_gen.store(self.curr_gen, Ordering::Release);
            Arc::clone(&entry.value)
        })
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get_or_compute<F>(&self, key: K, func: F) -> Arc<V>
    where
        F: FnOnce() -> (V, usize),
    {
        if let Some(entry) = self.try_get(&key) {
            return entry;
        }

        let timer = Instant::now();
        let (computed, memory_cost) = func();
        let computed = Arc::new(computed);
        let elapsed = timer.elapsed();
        let computation_cost = elapsed.as_nanos() as usize;
        {
            let mut new_entries = self.new_entries.lock().unwrap();
            new_entries.push_back((
                key,
                Entry {
                    computation_cost,
                    memory_cost,
                    cost_ratio: (computation_cost as f64) / (memory_cost as f64),
                    last_use_gen: AtomicUsize::new(self.curr_gen),
                    value: Arc::clone(&computed),
                },
            ));
        }
        computed
    }

    pub fn update(&mut self) {
        // Move all `self.new_entries` into `self.entries`. Accumulation of
        // `self.total_memory_cost` also happens here.
        for (k, v) in self.new_entries.lock().unwrap().drain(..) {
            match self.entries.entry(k) {
                e @ std::collections::btree_map::Entry::Vacant(_) => {
                    self.total_memory_cost += v.memory_cost;
                    e.insert_entry(v);
                }
                std::collections::btree_map::Entry::Occupied(_) => {
                    // Just drop a duplicate cache entry.
                }
            }
        }

        // We have drained the `new_entries`
        self.new_memory_cost = 0;

        if self.total_memory_cost > self.max_memory_cost {
            // NOTE: This might not be very efficient, especially with float total_cmp sort.
            //       For now, we drop more than strictly necessary, just so that we don't hit
            //       this operation too often.

            // Map entries to something we can sort
            let mut entries: Vec<_> = self
                .entries
                .iter()
                .map(|(k, v)| {
                    // Value is proportional to the computation cost and inversely proportional
                    // to both memory cost and age. Add 1 to age to prevent division by 0.
                    let age = self.curr_gen - v.last_use_gen.load(Ordering::Acquire);
                    let value = v.cost_ratio / (age + 1) as f64;
                    (k, value, v.memory_cost)
                })
                .collect();

            // We could maybe do something cheaper with partitions but this whole operation
            // is pretty bad either way.
            entries.sort_unstable_by(|lhs, rhs| lhs.1.total_cmp(&rhs.1));

            // Since the keys may actually be quite large we keep them as references
            // as long as possible. We have to first gather the keys to remove
            // by copying them before we can modify `self.entries`, this is reasonable
            // because we expect few removals compared to the number of entries.
            let mut keys_to_remove = Vec::new();
            for (key, _value, memory_cost) in entries {
                keys_to_remove.push(key.clone());
                self.total_memory_cost -= memory_cost;

                // Drop more than necessary to avoid doing this costly operation too often.
                if self.total_memory_cost <= self.max_memory_cost * 3 / 4 {
                    break;
                }
            }

            // Actually drop the entries
            for key in keys_to_remove {
                self.entries.remove(&key);
            }
        }

        self.curr_gen += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Barrier};
    use std::thread;

    fn make_cache(max_memory: usize) -> LockStepCache<u32, String> {
        LockStepCache::new(max_memory)
    }

    // ── Basic functionality ──────────────────────────────────────────────────

    #[test]
    fn miss_computes_and_returns_value() {
        let cache = make_cache(1000);
        let v = cache.get_or_compute(1, || ("hello".to_string(), 10));
        assert_eq!(*v, "hello");
    }

    #[test]
    fn hit_after_update_returns_same_value() {
        let mut cache = make_cache(1000);
        let v1 = cache.get_or_compute(1, || ("hello".to_string(), 10));
        cache.update();
        let v2 = cache.get_or_compute(1, || panic!("should not recompute"));
        assert!(Arc::ptr_eq(&v1, &v2), "should return the same Arc");
    }

    #[test]
    fn miss_before_update_does_not_hit() {
        // new_entries haven't been flushed yet, so a second call before
        // update() will compute again (duplicate is silently dropped in update).
        let cache = make_cache(1000);
        let mut compute_count = 0;
        cache.get_or_compute(1, || {
            compute_count += 1;
            ("a".to_string(), 10)
        });
        cache.get_or_compute(1, || {
            compute_count += 1;
            ("a".to_string(), 10)
        });
        assert_eq!(compute_count, 2, "both calls miss before update");
    }

    #[test]
    fn different_keys_are_independent() {
        let mut cache = make_cache(1000);
        cache.get_or_compute(1, || ("one".to_string(), 10));
        cache.get_or_compute(2, || ("two".to_string(), 10));
        cache.update();
        let v1 = cache.get_or_compute(1, || panic!("recomputed 1"));
        let v2 = cache.get_or_compute(2, || panic!("recomputed 2"));
        assert_eq!(*v1, "one");
        assert_eq!(*v2, "two");
    }

    #[test]
    fn try_get_misses_before_update() {
        let cache = make_cache(1000);
        cache.get_or_compute(42, || ("x".to_string(), 1));
        // Not yet flushed to `entries`
        assert!(!cache.contains_key(&42));
    }

    #[test]
    fn try_get_hits_after_update() {
        let mut cache = make_cache(1000);
        cache.get_or_compute(42, || ("x".to_string(), 1));
        cache.update();
        assert!(cache.contains_key(&42));
    }

    #[test]
    fn duplicate_keys_in_new_entries_keeps_first() {
        // Two computes before update — the second should be silently dropped.
        let mut cache = make_cache(1000);
        cache.get_or_compute(1, || ("first".to_string(), 10));
        cache.get_or_compute(1, || ("second".to_string(), 10));
        cache.update();
        let v = cache.try_get(&1).unwrap();
        assert_eq!(*v, "first");
    }

    #[test]
    fn total_memory_cost_accumulates_correctly() {
        let mut cache = make_cache(1000);
        cache.get_or_compute(1, || ("a".to_string(), 30));
        cache.get_or_compute(2, || ("b".to_string(), 50));
        cache.update();
        assert_eq!(cache.total_memory_cost, 80);
    }

    #[test]
    fn update_increments_generation() {
        let mut cache = make_cache(1000);
        assert_eq!(cache.curr_gen, 0);
        cache.update();
        assert_eq!(cache.curr_gen, 1);
        cache.update();
        assert_eq!(cache.curr_gen, 2);
    }

    #[test]
    fn eviction_brings_memory_under_budget() {
        // Budget: 50. Insert three entries totaling 90. After update,
        // the cache must evict enough to get back to ≤ 50.
        let mut cache = make_cache(50);
        cache.get_or_compute(1, || ("a".to_string(), 30));
        cache.get_or_compute(2, || ("b".to_string(), 30));
        cache.get_or_compute(3, || ("c".to_string(), 30));
        cache.update();
        assert!(
            cache.total_memory_cost <= 50,
            "cost={}",
            cache.total_memory_cost
        );

        // What we *can* assert: at least one entry was evicted.
        let present: usize = [1u32, 2, 3]
            .iter()
            .filter(|k| cache.contains_key(k))
            .count();
        assert!(present < 3, "expected eviction but all entries remain");
    }

    #[test]
    fn eviction_prefers_cheap_to_compute_entries() {
        // High cost_ratio = expensive to compute, cheap memory → keep.
        // Low cost_ratio = cheap to compute, expensive memory → evict first.
        //
        // cost_ratio = computation_cost / memory_cost.
        // We can't control computation_cost directly (it's wall time), but we
        // can verify that a high-memory, presumably low-ratio entry is evicted
        // before a low-memory one. This is a structural/smoke test.
        let mut cache: LockStepCache<u32, String> = LockStepCache::new(10000);
        // Very high memory cost to prevent spurious timing issues
        cache.get_or_compute(1, || ("expensive_memory".to_string(), 9999));
        cache.get_or_compute(2, || ("cheap_memory".to_string(), 1));
        cache.update();
        // Now add one more to trigger eviction.
        cache.get_or_compute(3, || ("one_more".to_string(), 5));
        cache.update();
        // Entry 1 (cost 9999) should be the first candidate for eviction.
        assert!(
            cache.contains_key(&2),
            "cheap-memory entry should survive eviction"
        );
    }

    // ── Concurrency ──────────────────────────────────────────────────────────

    #[test]
    fn concurrent_gets_do_not_panic() {
        // Multiple threads calling get_or_compute simultaneously must not
        // deadlock or panic. Duplicate computation is acceptable.
        let cache = Arc::new(make_cache(10_000));
        let barrier = Arc::new(Barrier::new(8));

        let handles: Vec<_> = (0u32..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    for k in 0u32..20 {
                        cache.get_or_compute(k, || (k.to_string(), 1));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }

    #[test]
    fn returned_arcs_remain_valid_after_further_inserts() {
        // Verify that holding an Arc<V> while the cache grows doesn't
        // invalidate the value (tests that Box removal didn't break stability).
        let cache = make_cache(10_000);
        let v1 = cache.get_or_compute(1, || ("first".to_string(), 1));
        for i in 2u32..200 {
            cache.get_or_compute(i, || (i.to_string(), 1));
        }
        assert_eq!(*v1, "first", "Arc value corrupted after many inserts");
    }
}
