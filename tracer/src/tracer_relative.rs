// Copyright © 2022 Intel Corporation
//
// SPDX-License-Identifier: Apache-2.0
//

use once_cell::unsync::OnceCell;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct Tracer {
    events: Arc<Mutex<HashMap<String, Vec<TraceEvent>>>>,
    thread_depths: HashMap<String, Arc<AtomicU64>>,
    start: Instant,
}

impl Tracer {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(HashMap::default())),
            start: Instant::now(),
            thread_depths: HashMap::default(),
        }
    }

    fn end(&self) {
        let end = Instant::now();
        // SAFETY: FFI call
        let path = format!("cloud-hypervisor-{}.virtio-mem-trace", unsafe {
            libc::getpid()
        });
        let mut file = File::create(&path).unwrap();

        #[derive(Serialize)]
        struct TraceReport {
            duration: Duration,
            events: Arc<Mutex<HashMap<String, Vec<TraceEvent>>>>,
        }

        let trace_report = TraceReport {
            duration: end.duration_since(self.start),
            events: self.events.clone(),
        };

        serde_json::to_writer_pretty(&file, &trace_report).unwrap();

        file.flush().unwrap();

        warn!("Trace output: {}", path);
    }

    fn add_event(&mut self, event: TraceEvent) {
        let current = std::thread::current();
        let thread_name = current.name().unwrap_or("");
        let mut events = self.events.lock().unwrap();
        if let Some(thread_events) = events.get_mut(thread_name) {
            thread_events.push(event);
        } else {
            events.insert(thread_name.to_string(), vec![event]);
        }
    }

    fn increase_thread_depth(&mut self) {
        let current = std::thread::current();
        let thread_name = current.name().unwrap_or("");
        if let Some(depth) = self.thread_depths.get_mut(thread_name) {
            depth.fetch_add(1, Ordering::SeqCst);
        } else {
            self.thread_depths
                .insert(thread_name.to_string(), Arc::new(AtomicU64::new(0)));
        }
    }

    fn decrease_thread_depth(&mut self) {
        let current = std::thread::current();
        let thread_name = current.name().unwrap_or("");
        if let Some(depth) = self.thread_depths.get_mut(thread_name) {
            depth.fetch_sub(1, Ordering::SeqCst);
        } else {
            panic!("Unmatched decrease for thread: {thread_name}");
        }
    }

    fn thread_depth(&self) -> u64 {
        let current = std::thread::current();
        let thread_name = current.name().unwrap_or("");
        self.thread_depths
            .get(thread_name)
            .map(|v| v.load(Ordering::SeqCst))
            .unwrap_or_default()
    }
}

static mut TRACER: OnceCell<Tracer> = OnceCell::new();

#[derive(Clone, Debug, Serialize)]
struct TraceEvent {
    timestamp: Duration,
    event: &'static str,
    end_timestamp: u128,
    depth: u64,
    size: Option<u64>,
    plug: Option<bool>
}

pub fn trace_point_log(event: &'static str) {
    let trace_event = TraceEvent {
        // SAFETY: start has been initialised as part of initialising the value of TRACER
        timestamp: Instant::now().duration_since(unsafe { TRACER.get().unwrap().start }),
        event,
        end_timestamp: 0,
        // SAFETY: thread_depth accesses current thread only specific data
        depth: unsafe { TRACER.get().unwrap().thread_depth() },
        size: None,
        plug: None
    };
    // SAFETY: add_event accesses current thread only specific data
    unsafe {
        TRACER.get_mut().unwrap().add_event(trace_event);
    }
}

pub struct TraceBlock {
    start: Instant,
    event: &'static str,
    size: u64,
    plug: bool
}

impl TraceBlock {
    pub fn new(event: &'static str, size: u64, plug: bool) -> Self {
        // SAFETY: increase_thread_depth accesses current thread only specific data
        unsafe {
            TRACER.get_mut().unwrap().increase_thread_depth();
        }
        Self {
            start: Instant::now(),
            event,
            size,
            plug
        }
    }
}

fn monotonic_clock() -> u128 {
    use nix::time;
    let ts = time::clock_gettime(time::ClockId::CLOCK_MONOTONIC).unwrap();
    (ts.tv_sec() * 1_000_000_000 + ts.tv_nsec()) as u128
}

impl Drop for TraceBlock {
    fn drop(&mut self) {
        // SAFETY: start has been initialised as part of initialising the value of TRACER
        let start = unsafe { TRACER.get().unwrap().start };
        let trace_event = TraceEvent {
            timestamp: self.start.duration_since(start),
            event: self.event,
            end_timestamp: monotonic_clock(),
            // SAFETY: thread_depth() returns a number local to the current thread
            depth: unsafe { TRACER.get().unwrap().thread_depth() },
            size: Some(self.size),
            plug: Some(self.plug)
        };
        // SAFETY: add_event and decrease_thread_depth access current thread only specific data
        unsafe {
            TRACER.get_mut().unwrap().add_event(trace_event);
            TRACER.get_mut().unwrap().decrease_thread_depth();
        }
    }
}

#[macro_export]
macro_rules! trace_relative_point {
    ($event:expr) => {
        $crate::trace_point_log($event)
    };
}

#[macro_export]
macro_rules! trace_relative_scoped {
    ($event:expr, $size:expr, $plug:expr) => {
        let _trace_scoped = $crate::TraceBlock::new($event, $size, $plug);
    };
}

pub fn end_relative() {
    // SAFETY: this is called after all other threads end
    unsafe { TRACER.get().unwrap().end() }
}

pub fn start_relative() {
    // SAFETY: this is called before other threads start
    unsafe { TRACER.set(Tracer::new()).unwrap() }
}
