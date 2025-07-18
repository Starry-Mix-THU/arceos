use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once};

use crate::{WaitQueue, api as axtask, current};

static INIT: Once = Once::new();

static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn test_sched_fifo() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 10;
    static FINISHED_TASKS: AtomicUsize = AtomicUsize::new(0);

    for i in 0..NUM_TASKS {
        axtask::spawn_raw(
            move || {
                println!("sched-fifo: Hello, task {}! ({})", i, current().id_name());
                axtask::yield_now();
                let order = FINISHED_TASKS.fetch_add(1, Ordering::Release);
                assert_eq!(order, i); // FIFO scheduler
                axtask::exit(0);
            },
            format!("T{i}"),
            0x1000,
        );
    }

    while FINISHED_TASKS.load(Ordering::Acquire) < NUM_TASKS {
        axtask::yield_now();
    }
}

#[test]
fn test_fp_state_switch() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 5;
    const FLOATS: [f64; NUM_TASKS] = [
        core::f64::consts::PI,
        core::f64::consts::E,
        -core::f64::consts::SQRT_2,
        0.0,
        0.618033988749895,
    ];
    static FINISHED_TASKS: AtomicUsize = AtomicUsize::new(0);

    for (i, float) in FLOATS.iter().enumerate() {
        axtask::spawn(move || {
            let mut value = float + i as f64;
            axtask::yield_now();
            value -= i as f64;

            println!("fp_state_switch: Float {i} = {value}");
            assert!((value - float).abs() < 1e-9);
            FINISHED_TASKS.fetch_add(1, Ordering::Release);
            axtask::exit(0);
        });
    }
    while FINISHED_TASKS.load(Ordering::Acquire) < NUM_TASKS {
        axtask::yield_now();
    }
}

#[test]
fn test_wait_queue() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 10;

    static WQ1: WaitQueue = WaitQueue::new();
    static WQ2: WaitQueue = WaitQueue::new();
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    for _ in 0..NUM_TASKS {
        axtask::spawn(move || {
            COUNTER.fetch_add(1, Ordering::Release);
            println!("wait_queue: task {:?} started", current().id());
            WQ1.notify_one(true); // WQ1.wait_until()
            WQ2.wait();

            assert!(!current().in_wait_queue());

            COUNTER.fetch_sub(1, Ordering::Release);
            println!("wait_queue: task {:?} finished", current().id());
            WQ1.notify_one(true); // WQ1.wait_until()
            axtask::exit(0);
        });
    }

    println!("task {:?} is waiting for tasks to start...", current().id());
    WQ1.wait_until(|| COUNTER.load(Ordering::Acquire) == NUM_TASKS);
    assert_eq!(COUNTER.load(Ordering::Acquire), NUM_TASKS);
    assert!(!current().in_wait_queue());
    WQ2.notify_all(true); // WQ2.wait()

    println!(
        "task {:?} is waiting for tasks to finish...",
        current().id()
    );
    WQ1.wait_until(|| COUNTER.load(Ordering::Acquire) == 0);
    assert_eq!(COUNTER.load(Ordering::Acquire), 0);
    assert!(!current().in_wait_queue());
}

#[test]
fn test_task_join() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 10;
    let mut tasks = Vec::with_capacity(NUM_TASKS);

    for i in 0..NUM_TASKS {
        tasks.push(axtask::spawn_raw(
            move || {
                println!("task_join: task {}! ({})", i, current().id_name());
                axtask::yield_now();
                axtask::exit(i as _);
            },
            format!("T{i}"),
            0x1000,
        ));
    }

    for (i, task) in tasks.into_iter().enumerate() {
        assert_eq!(task.join(), i as _);
    }
}
