use crate::result::Result;

use core::cell::SyncUnsafeCell;
use core::fmt::Debug;
use core::ops::Deref;
use core::ops::DerefMut;
use core::panic::Location;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

pub struct MutexGuard<'a, T> {
    lock: &'a Mutex<T>,
    data: &'a mut T,
    location: Location<'a>,
}

impl<'a, T> MutexGuard<'a, T> {
    #[track_caller]
    unsafe fn new(mutex: &'a Mutex<T>, data: &SyncUnsafeCell<T>) -> Self {
        Self {
            lock: mutex,
            data: &mut *data.get(),
            location: *Location::caller(),
        }
    }
}

unsafe impl<'a, T> Sync for MutexGuard<'a, T> {}
impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::SeqCst);
    }
}

impl<'a, T> Debug for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "MutexGuard @ {}:{}, taken at {}:{}",
            self.lock.created_at_file,
            self.lock.created_at_line,
            self.location.file(),
            self.location.line()
        )
    }
}

pub struct Mutex<T> {
    data: SyncUnsafeCell<T>,
    locked: AtomicBool,
    taker_line_num: AtomicU32,
    created_at_file: &'static str,
    created_at_line: u32,
}

impl<T> Debug for Mutex<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Mutex @ {}:{}",
            self.created_at_file, self.created_at_line
        )
    }
}

impl<T> Mutex<T> {
    #[track_caller]
    pub const fn new(data: T) -> Self {
        let location = Location::caller();
        Mutex {
            data: SyncUnsafeCell::new(data),
            locked: AtomicBool::new(false),
            taker_line_num: AtomicU32::new(0),
            created_at_file: location.file(),
            created_at_line: location.line(),
        }
    }

    #[track_caller]
    fn try_lock(&self) -> Result<MutexGuard<T>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.taker_line_num
                .store(Location::caller().line(), Ordering::SeqCst);
            Ok(unsafe { MutexGuard::new(self, &self.data) })
        } else {
            Err("Locke failed")
        }
    }

    #[track_caller]
    pub fn lock(&self) -> MutexGuard<T> {
        for _ in 0..100000 {
            if let Ok(locked) = self.try_lock() {
                return locked;
            }
        }
        panic!(
            "Failed to lock Mutex at {}:{}, caller: {:?}, taker_line_num: {}",
            self.created_at_file,
            self.created_at_line,
            Location::caller(),
            self.taker_line_num.load(Ordering::SeqCst),
        )
    }

    #[track_caller]
    pub fn under_locked<R>(&self, f: &dyn Fn(&mut T) -> Result<R>) -> Result<R> {
        let mut guard = self.lock();
        f(&mut *guard)
    }
}

unsafe impl<T> Sync for Mutex<T> {}
impl<T: Default> Default for Mutex<T> {
    #[track_caller]
    fn default() -> Self {
        Self::new(T::default())
    }
}
