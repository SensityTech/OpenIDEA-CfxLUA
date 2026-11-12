//! What each part of the plugin keeps in memory once a project is read. Built with `--features mem-trace`,
//! every allocation is counted and the log line that ends the scan lists what was kept, part by part;
//! without it nothing is counted and nothing is logged.

#[cfg(feature = "mem-trace")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicIsize, Ordering};

    static LIVE: AtomicIsize = AtomicIsize::new(0);
    /// The most that was allocated at once, and since the last part was counted.
    static PEAK: AtomicIsize = AtomicIsize::new(0);
    static PEAK_SINCE: AtomicIsize = AtomicIsize::new(0);

    fn grow(bytes: isize) {
        let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
        PEAK.fetch_max(live, Ordering::Relaxed);
        PEAK_SINCE.fetch_max(live, Ordering::Relaxed);
    }

    struct Counting;

    // SAFETY: every call is passed on to the system allocator unchanged; only sizes are counted.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            grow(layout.size() as isize);
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            grow(new_size as isize - layout.size() as isize);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    thread_local! {
        static KEPT: RefCell<Vec<(String, isize)>> = const { RefCell::new(Vec::new()) };
        /// The part that was being counted when the most was allocated, and how much it held above what
        /// was allocated when it started.
        static HIGHEST: RefCell<(String, isize, isize)> = const { RefCell::new((String::new(), 0, 0)) };
    }

    pub fn live() -> isize {
        LIVE.load(Ordering::Relaxed)
    }

    pub fn keep(part: &str, since: isize) {
        let reached = PEAK_SINCE.swap(live(), Ordering::Relaxed);
        HIGHEST.with_borrow_mut(|highest| {
            if reached > highest.1 {
                *highest = (part.to_owned(), reached, reached - since);
            }
        });
        let grown = live() - since;
        KEPT.with_borrow_mut(|kept| match kept.iter_mut().find(|(name, _)| name == part) {
            Some((_, bytes)) => *bytes += grown,
            None => kept.push((part.to_owned(), grown)),
        });
    }

    pub fn report() -> String {
        let mut parts = KEPT.with_borrow(|kept| kept.clone());
        parts.sort_by_key(|(_, bytes)| -bytes);
        let listed: Vec<String> = parts.iter().map(|(name, bytes)| format!("{name} {} KB", bytes / 1024)).collect();
        let (part, reached, above) = HIGHEST.with_borrow(|highest| highest.clone());
        let peak = PEAK.load(Ordering::Relaxed) / 1024;
        let highest = format!(
            "{} KB at most, {} KB reached in {part} which held {} KB of it",
            peak,
            reached / 1024,
            above / 1024
        );
        format!("; {} KB allocated ({highest}): {}", live() / 1024, listed.join(", "))
    }
}

/// Whether allocations are counted.
pub const COUNTING: bool = cfg!(feature = "mem-trace");

/// How many times a file costs what it usually does to index: counted, it is analysed once per feature.
pub const COST: usize = if COUNTING { 16 } else { 1 };

/// The bytes allocated at this moment; 0 when allocations are not counted.
pub fn live() -> isize {
    #[cfg(feature = "mem-trace")]
    return counting::live();
    #[cfg(not(feature = "mem-trace"))]
    0
}

/// Counts for `part` what was allocated and not freed since `since`, a value of `live`.
pub fn keep(part: &str, since: isize) {
    #[cfg(feature = "mem-trace")]
    counting::keep(part, since);
    #[cfg(not(feature = "mem-trace"))]
    let _ = (part, since);
}

/// What each part kept, for the log; empty when allocations are not counted.
pub fn report() -> String {
    #[cfg(feature = "mem-trace")]
    return counting::report();
    #[cfg(not(feature = "mem-trace"))]
    String::new()
}
