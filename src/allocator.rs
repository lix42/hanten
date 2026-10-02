//! The process's global allocator: the system allocator, except that a block of
//! [`DIRECT_MIN_BYTES`] or more is mapped from the OS directly and unmapped on free
//! (`io/multi-frame-memory-growth`).
//!
//! **Why.** macOS malloc keeps freed large blocks resident for reuse, and the next
//! frame of a roll is a few pixels larger and cannot reuse them, so a multi-frame
//! run's resident set climbed with every frame (`vmmap`: `MALLOC_LARGE (empty)`) —
//! the numbers are in `pipeline::memory`. `malloc_zone_pressure_relief` releases
//! none of it. glibc maps blocks over its mmap threshold (at most 32 MiB) directly
//! already, so on Linux this changes only the 8–32 MiB band.
//!
//! **Cost:** no freed big block is reused, so each faults in fresh zeroed pages. Only
//! `measure-roll`, the lightest per frame, shows it in wall time (measured in
//! `docs/progress/io.md`). Mapped pages are already zero, so `alloc_zeroed` skips the
//! memset; growth on macOS copies, having no `mremap` ([`resize`]).
//!
//! The route is a pure function of the [`Layout`] ([`is_direct`]), so `dealloc` and
//! `realloc`, which receive the allocating layout, always find the path a block took.

use std::alloc::{GlobalAlloc, Layout, System};

/// Blocks at least this large bypass malloc. Every full-frame plane of a 4.2 MP or
/// larger frame is over it (the smallest, the `u16` IR read buffer, is 2 B/px).
pub const DIRECT_MIN_BYTES: usize = 8 << 20;

/// Alignments a fresh mapping always satisfies: mappings are page-aligned, and no
/// supported target has pages under 4 KiB.
const DIRECT_MAX_ALIGN: usize = 4096;

/// System malloc for small blocks, the OS's page mapper for big ones.
pub struct Allocator;

/// Whether `layout` is served by a direct mapping rather than malloc.
pub fn is_direct(layout: Layout) -> bool {
    cfg!(unix) && layout.size() >= DIRECT_MIN_BYTES && layout.align() <= DIRECT_MAX_ALIGN
}

// SAFETY: small blocks are System's, untouched. A direct block is a private anonymous
// mapping of exactly `layout.size()` bytes, returned to the caller whole and unmapped
// only by `dealloc`/`realloc` with that same layout; `is_direct` routes both ends the
// same way. Nothing here panics or allocates.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if is_direct(layout) {
            map(layout.size())
        } else {
            // SAFETY: the caller's contract for `alloc` is System's.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if is_direct(layout) {
            map(layout.size())
        } else {
            // SAFETY: as for `alloc`.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if is_direct(layout) {
            // SAFETY: `layout` allocated `ptr`, so `alloc` mapped it at this size.
            unsafe { unmap(ptr, layout.size()) }
        } else {
            // SAFETY: as above, and System allocated it.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees `new_size` rounded to `layout.align()` fits.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (is_direct(layout), is_direct(new_layout)) {
            // SAFETY: both ends are System's.
            (false, false) => return unsafe { System.realloc(ptr, layout, new_size) },
            (true, true) => {
                // SAFETY: `ptr` is a direct block of `layout.size()` bytes.
                if let Some(p) = unsafe { resize(ptr, layout.size(), new_size) } {
                    return p;
                }
            }
            _ => {}
        }
        // SAFETY: `new_layout` is non-zero (the caller's `new_size > 0`); on success the
        // blocks are distinct, `ptr` is valid for `layout.size()` reads and the new block
        // for `new_size` writes, and `ptr` is freed with the layout that allocated it.
        unsafe {
            let new_ptr = self.alloc(new_layout);
            if !new_ptr.is_null() {
                std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
            new_ptr
        }
    }
}

/// A fresh private anonymous mapping of `size` bytes, zeroed by the OS; null on failure.
#[cfg(unix)]
fn map(size: usize) -> *mut u8 {
    // SAFETY: a null hint and an anonymous private mapping touch no existing memory.
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        std::ptr::null_mut()
    } else {
        p.cast()
    }
}

/// Resize a direct block without copying where the OS allows: a shrink, or growth
/// within its last page, keeps the block (unmapping any freed tail pages), and Linux
/// grows it with `mremap`. `None` leaves the block untouched for the caller to copy;
/// `Some(null)` is a failed `mremap`, which also leaves it untouched.
///
/// # Safety
///
/// `ptr` must come from `map(old)` and not have been unmapped.
#[cfg(unix)]
unsafe fn resize(ptr: *mut u8, old: usize, new: usize) -> Option<*mut u8> {
    let page = page_size();
    let (old_pages, new_pages) = (
        old.checked_next_multiple_of(page)?,
        new.checked_next_multiple_of(page)?,
    );
    if new_pages <= old_pages {
        if new_pages < old_pages {
            // SAFETY: the tail lies inside the caller's mapping and past `new` bytes,
            // so nothing the caller keeps is in it; a later `unmap(ptr, new)` covers
            // exactly the pages that remain.
            unsafe { unmap(ptr.add(new_pages), old_pages - new_pages) };
        }
        return Some(ptr);
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `ptr` maps `old_pages`; `MREMAP_MAYMOVE` moves the pages, contents
        // intact, or fails leaving the mapping as it was.
        let p = unsafe { libc::mremap(ptr.cast(), old_pages, new_pages, libc::MREMAP_MAYMOVE) };
        Some(if p == libc::MAP_FAILED {
            std::ptr::null_mut()
        } else {
            p.cast()
        })
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// The OS page size, read once; mappings are whole pages of it.
#[cfg(unix)]
fn page_size() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static PAGE: AtomicUsize = AtomicUsize::new(0);
    match PAGE.load(Ordering::Relaxed) {
        0 => {
            // SAFETY: `sysconf` reads a constant; it allocates nothing.
            let p = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
                .ok()
                .filter(|&p| p >= DIRECT_MAX_ALIGN && p.is_power_of_two())
                .unwrap_or(DIRECT_MAX_ALIGN);
            PAGE.store(p, Ordering::Relaxed);
            p
        }
        p => p,
    }
}

/// Unmap a block [`map`] returned for the same `size`.
///
/// # Safety
///
/// `ptr` must come from `map(size)` and not have been unmapped.
#[cfg(unix)]
unsafe fn unmap(ptr: *mut u8, size: usize) {
    // SAFETY: the caller's contract. A failure would mean the range was not ours,
    // which that contract rules out; there is nothing to recover, so it is ignored.
    unsafe { libc::munmap(ptr.cast(), size) };
}

// `is_direct` is never true off unix; these exist so the dispatch type-checks there.
#[cfg(not(unix))]
fn map(_size: usize) -> *mut u8 {
    std::ptr::null_mut()
}

#[cfg(not(unix))]
unsafe fn unmap(_ptr: *mut u8, _size: usize) {}

#[cfg(not(unix))]
unsafe fn resize(_ptr: *mut u8, _old: usize, _new: usize) -> Option<*mut u8> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_depends_on_size_and_alignment_only() {
        let big = Layout::from_size_align(DIRECT_MIN_BYTES, 8).unwrap();
        let small = Layout::from_size_align(DIRECT_MIN_BYTES - 1, 8).unwrap();
        let over_aligned = Layout::from_size_align(DIRECT_MIN_BYTES, 2 * DIRECT_MAX_ALIGN).unwrap();
        assert_eq!(is_direct(big), cfg!(unix));
        assert!(!is_direct(small));
        assert!(!is_direct(over_aligned));
    }

    /// Through the registered allocator: zeroed, writable, and contents kept across a
    /// `realloc` in each direction over the threshold.
    #[test]
    fn a_vec_crossing_the_threshold_keeps_its_contents() {
        let n = DIRECT_MIN_BYTES / 4;
        let zeros = vec![0f32; n];
        assert!(zeros.iter().all(|&v| v == 0.0));

        let mut v: Vec<u32> = (0..(n as u32 / 2)).collect();
        v.extend(n as u32 / 2..n as u32 + 7); // small → direct
        assert!(v.iter().enumerate().all(|(i, &x)| x == i as u32));
        v.truncate(10);
        v.shrink_to_fit(); // direct → small
        assert_eq!(v, (0..10).collect::<Vec<u32>>());

        let mut w: Vec<u8> = vec![7; DIRECT_MIN_BYTES];
        w.resize(2 * DIRECT_MIN_BYTES, 9); // direct → direct, growing
        assert!(w[..DIRECT_MIN_BYTES].iter().all(|&b| b == 7));
        assert!(w[DIRECT_MIN_BYTES..].iter().all(|&b| b == 9));
        w.truncate(DIRECT_MIN_BYTES + 100);
        w.shrink_to_fit(); // direct → direct, shrinking in place
        assert_eq!(w.capacity(), DIRECT_MIN_BYTES + 100);
        assert!(w[..DIRECT_MIN_BYTES].iter().all(|&b| b == 7));
        assert!(w[DIRECT_MIN_BYTES..].iter().all(|&b| b == 9));
        w.reserve_exact(100); // grows within its last page
        w.resize(DIRECT_MIN_BYTES + 200, 5);
        assert!(w[DIRECT_MIN_BYTES + 100..].iter().all(|&b| b == 5));
    }
}
