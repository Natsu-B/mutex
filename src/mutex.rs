use std::cell::SyncUnsafeCell;
use std::cell::UnsafeCell;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

static RAW_ATOMICS_ENABLED: SyncUnsafeCell<bool> = SyncUnsafeCell::new(false);

#[inline]
pub fn raw_atomics_enabled() -> bool {
    // SAFETY: `RAW_ATOMICS_ENABLED` is only written during single-std bring-up,
    // and reads are allowed as a simple flag check after that point.
    unsafe { *RAW_ATOMICS_ENABLED.get() }
}

/// Enables raw atomic operations globally for this crate.
///
/// # Invariants
/// - Call only after paging/caches/memory attributes are enabled. If called too early,
///   subsequent lock/atomic operations will execute atomic RMW instructions while the
///   platform still forbids them, which can trap or lead to unpredictable memory behavior.
/// - Call before secondary stds start and before any concurrent lock usage. If called
///   concurrently with readers, some operations may remain non-atomic while others become
///   atomic, leading to data races, lost updates, or aliasing UB.
/// - This function is intentionally unsynchronized and must only run during single-std
///   bring-up to avoid races with `raw_atomics_enabled()` readers.
#[inline]
pub fn enable_raw_atomics() {
    // SAFETY: callers must uphold the bring-up sequencing and single-std invariants above.
    unsafe {
        *RAW_ATOMICS_ENABLED.get() = true;
    }
}

#[inline]
fn disable_raw_atomics() {
    // SAFETY: used only in tests to restore state.
    unsafe {
        *RAW_ATOMICS_ENABLED.get() = false;
    }
}

#[inline(always)]
fn lock_atomic(locked: &AtomicBool) {
    while locked
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
}

#[inline(always)]
fn unlock_atomic(locked: &AtomicBool) {
    locked.store(false, Ordering::Release);
}

const WRITE_FLAG: usize = 1 << (usize::BITS - 1);

#[inline(always)]
fn rw_read_lock_atomic(state: &AtomicUsize) {
    loop {
        let current_state = state.load(Ordering::Relaxed);
        if current_state & WRITE_FLAG != 0 {
            std::hint::spin_loop();
            continue;
        }

        let next_state = match current_state.checked_add(1) {
            Some(next) => next,
            None => {
                std::hint::spin_loop();
                continue;
            }
        };

        if next_state & WRITE_FLAG != 0 {
            std::hint::spin_loop();
            continue;
        }

        if state
            .compare_exchange_weak(
                current_state,
                next_state,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            break;
        }
        std::hint::spin_loop();
    }
}

#[inline(always)]
fn rw_read_unlock_atomic(state: &AtomicUsize) {
    state.fetch_sub(1, Ordering::Release);
}

#[inline(always)]
fn rw_write_lock_atomic(state: &AtomicUsize) {
    loop {
        let current_state = state.load(Ordering::Relaxed);
        if current_state & WRITE_FLAG != 0 {
            std::hint::spin_loop();
            continue;
        }

        if state
            .compare_exchange_weak(
                current_state,
                current_state | WRITE_FLAG,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            while state.load(Ordering::Relaxed) & !WRITE_FLAG != 0 {
                std::hint::spin_loop();
            }
            break;
        }
        std::hint::spin_loop();
    }
}

#[inline(always)]
fn rw_write_unlock_atomic(state: &AtomicUsize) {
    state.fetch_and(!WRITE_FLAG, Ordering::Release);
}

pub struct RawSpinLock<T: ?Sized> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

pub struct RawSpinLockGuard<'a, T> {
    lock: &'a RawSpinLock<T>,
    unlock_on_drop: bool,
}

unsafe impl<T: ?Sized + Send> Send for RawSpinLock<T> {}
unsafe impl<T: ?Sized + Send> Sync for RawSpinLock<T> {}

impl<T> RawSpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> RawSpinLockGuard<'_, T> {
        let unlock_on_drop = raw_atomics_enabled();
        if unlock_on_drop {
            lock_atomic(&self.locked);
        }
        RawSpinLockGuard {
            lock: self,
            unlock_on_drop,
        }
    }

    /// Returns a guard without acquiring the lock or modifying lock state.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no other CPU/thread can access the protected
    /// value concurrently (including via [`lock`](Self::lock)), and that no
    /// other guard exists that could produce references to the same `T`.
    /// Breaking these requirements can cause data races or aliasing UB.
    pub unsafe fn no_lock(&self) -> RawSpinLockGuard<'_, T> {
        RawSpinLockGuard {
            lock: self,
            unlock_on_drop: false,
        }
    }
}

#[cfg(test)]
impl<T: ?Sized> RawSpinLock<T> {
    fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Acquire)
    }
}

impl<T> Drop for RawSpinLockGuard<'_, T> {
    fn drop(&mut self) {
        if self.unlock_on_drop {
            unlock_atomic(&self.lock.locked);
        }
    }
}

impl<T> Deref for RawSpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for RawSpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
