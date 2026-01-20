#![feature(sync_unsafe_cell)]

// バイナリセマフォを実現する

use std::{cell::SyncUnsafeCell, sync::atomic::AtomicUsize, thread, time::Duration};

use crate::mutex::RawSpinLock;

mod mutex;

static SEMAPHORE1: RawSpinLock<()> = RawSpinLock::new(());
static SEMAPHORE2: RawSpinLock<()> = RawSpinLock::new(());
static SEMAPHORE3: RawSpinLock<()> = RawSpinLock::new(());
static SEMAPHORE4: RawSpinLock<()> = RawSpinLock::new(());
static SEMAPHORE5: RawSpinLock<()> = RawSpinLock::new(());

static NON_ATOMIC_USIZE: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);
static ATOMIC_USIZE: AtomicUsize = AtomicUsize::new(0);

struct Philosopher<'a>(u8, &'a RawSpinLock<()>, &'a RawSpinLock<()>);

impl<'a> Philosopher<'a> {
    fn new(num: u8, left: &'a RawSpinLock<()>, right: &'a RawSpinLock<()>) -> Self {
        Philosopher(num, left, right)
    }

    fn eat(&self) {
        let _left = self.1.lock();
        let _right = self.2.lock();
        println!("philosopher {} eating...", self.0);
        thread::sleep(Duration::from_secs(1));
        println!("philosopher {} finished eating.", self.0);
    }
}

fn main() {
    let philosophers = vec![
        Philosopher::new(1, &SEMAPHORE1, &SEMAPHORE2),
        Philosopher::new(2, &SEMAPHORE2, &SEMAPHORE3),
        Philosopher::new(3, &SEMAPHORE3, &SEMAPHORE4),
        Philosopher::new(4, &SEMAPHORE4, &SEMAPHORE5),
        Philosopher::new(5, &SEMAPHORE5, &SEMAPHORE1),
    ];

    let handles: Vec<_> = philosophers
        .into_iter()
        .map(|p| {
            thread::spawn(move || {
                p.eat();
                for _ in 0..1000 {
                    unsafe { *NON_ATOMIC_USIZE.get() += 1 };
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    println!("NON_ATOMIC_USIZE: {}", unsafe { *NON_ATOMIC_USIZE.get() });

    mutex::enable_raw_atomics();
    println!("--- With raw atomics enabled ---");

    let philosophers = vec![
        Philosopher::new(1, &SEMAPHORE1, &SEMAPHORE2),
        Philosopher::new(2, &SEMAPHORE2, &SEMAPHORE3),
        Philosopher::new(3, &SEMAPHORE3, &SEMAPHORE4),
        Philosopher::new(4, &SEMAPHORE4, &SEMAPHORE5),
        Philosopher::new(5, &SEMAPHORE5, &SEMAPHORE1),
    ];

    let handles: Vec<_> = philosophers
        .into_iter()
        .map(|p| {
            thread::spawn(move || {
                p.eat();
                for _ in 0..1000 {
                    ATOMIC_USIZE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    println!(
        "ATOMIC_USIZE: {}",
        ATOMIC_USIZE.load(std::sync::atomic::Ordering::SeqCst)
    );
}
