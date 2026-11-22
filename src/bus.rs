//! 事件总线：单写多读（设计文档 §3.2）。
//!
//! - 写侧：单一生者（编排核心），递增 `tail` 发布。
//! - 读侧：每个订阅者一个环形缓冲游标；慢订阅者不阻塞生者。
//! - 游标落后超过容量即丢弃缺口，并记录 `subscriber.lagged`（订阅者自行从日志补齐）。
//! - 订阅者可声明只投递哪些 kind 的投影（WASM 订阅者只能拿到过滤后的投影）。
//!
//! 环槽存 `(环序号, 事件)`——游标对齐用的是环序号，绝不能用日志的 `seq`
//! （跨会话的 seq 会重叠）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::proto::types::{Event, EventKind};

pub struct Bus {
    cap: usize,
    slots: Vec<Mutex<Option<(u64, Arc<Event>)>>>,
    tail: AtomicU64,
    changed: Condvar,
    changed_lock: Mutex<()>,
    next_sub: AtomicU64,
    lagged_total: AtomicU64,
}

#[derive(Clone)]
pub struct Subscription {
    pub id: u64,
    pub cursor: u64,
    pub kinds: Option<Vec<EventKind>>,
}

#[derive(Debug, Default)]
pub struct PollOutcome {
    pub events: Vec<Arc<Event>>,
    pub lagged: u64,
}

impl Bus {
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(16);
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(Mutex::new(None));
        }
        Self {
            cap,
            slots,
            tail: AtomicU64::new(0),
            changed: Condvar::new(),
            changed_lock: Mutex::new(()),
            next_sub: AtomicU64::new(1),
            lagged_total: AtomicU64::new(0),
        }
    }

    pub fn tail(&self) -> u64 {
        self.tail.load(Ordering::Acquire)
    }

    pub fn lagged_total(&self) -> u64 {
        self.lagged_total.load(Ordering::Relaxed)
    }

    /// 发布一条事件；写者单线程调用。
    pub fn publish(&self, ev: Arc<Event>) {
        let t = self.tail.load(Ordering::Relaxed);
        let idx = (t as usize) % self.cap;
        *self.slots[idx].lock() = Some((t, ev));
        self.tail.store(t + 1, Ordering::Release);
        self.changed.notify_all();
    }

    pub fn subscribe(&self, kinds: Option<Vec<EventKind>>) -> Subscription {
        let id = self.next_sub.fetch_add(1, Ordering::Relaxed);
        Subscription {
            id,
            cursor: self.tail(),
            kinds,
        }
    }

    fn accepts(sub: &Subscription, kind: EventKind) -> bool {
        match &sub.kinds {
            None => true,
            Some(k) => k.contains(&kind),
        }
    }

    /// 拉取新事件（非阻塞）。落后超过容量则丢弃缺口并累计 lag。
    pub fn poll(&self, sub: &mut Subscription) -> PollOutcome {
        let mut out = PollOutcome::default();
        let tail = self.tail.load(Ordering::Acquire);
        let floor = tail.saturating_sub(self.cap as u64);
        if sub.cursor < floor {
            let dropped = floor - sub.cursor;
            out.lagged = dropped;
            self.lagged_total.fetch_add(dropped, Ordering::Relaxed);
            sub.cursor = floor;
            let lag = Event::new(
                0,
                0,
                EventKind::SubscriberLagged,
                serde_json::json!({"subscription": sub.id, "dropped": dropped}),
            );
            // 注意：发布后 tail 前进，本次仍按旧 tail 读取，下一轮再追
            self.publish(Arc::new(lag));
        }
        let mut cur = sub.cursor;
        while cur < tail {
            let idx = (cur as usize) % self.cap;
            let slot = self.slots[idx].lock();
            match &*slot {
                Some((ring_idx, ev)) if *ring_idx == cur => {
                    if Self::accepts(sub, ev.kind) {
                        out.events.push(ev.clone());
                    }
                    drop(slot);
                    cur += 1;
                }
                Some((ring_idx, _)) if *ring_idx > cur => {
                    // 槽已被后来的事件覆盖
                    let jump = *ring_idx;
                    drop(slot);
                    sub.cursor = sub.cursor.max(jump);
                    cur = jump;
                }
                _ => {
                    drop(slot);
                    cur += 1;
                }
            }
        }
        sub.cursor = sub.cursor.max(cur);
        out
    }

    /// 阻塞等待新事件到达（最多 `timeout`）。
    pub fn wait(&self, cursor: u64, timeout: Duration) -> bool {
        if self.tail() > cursor {
            return true;
        }
        let mut g = self.changed_lock.lock();
        if self.tail() > cursor {
            return true;
        }
        let _ = self.changed.wait_for(&mut g, timeout);
        drop(g);
        self.tail() > cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fanout_and_filter() {
        let bus = Bus::new(64);
        let mut a = bus.subscribe(None);
        let mut b = bus.subscribe(Some(vec![EventKind::ModelDelta]));
        let mut seqs = Vec::new();
        for i in 0..5u64 {
            let ev = Arc::new(Event::new(
                i + 1,
                i,
                if i % 2 == 0 {
                    EventKind::UserMessage
                } else {
                    EventKind::ModelDelta
                },
                serde_json::json!({"i": i}),
            ));
            seqs.push(bus.tail());
            bus.publish(ev);
        }
        let oa = bus.poll(&mut a);
        let ob = bus.poll(&mut b);
        assert_eq!(oa.events.len(), 5);
        assert_eq!(ob.events.len(), 2);
        assert_eq!(oa.lagged, 0);
    }

    #[test]
    fn slow_subscriber_lag_is_recorded() {
        let bus = Bus::new(8);
        let mut s = bus.subscribe(None);
        for i in 0..20u64 {
            bus.publish(Arc::new(Event::new(
                i + 1,
                i,
                EventKind::UserMessage,
                serde_json::json!({}),
            )));
        }
        let out = bus.poll(&mut s);
        assert!(out.lagged > 0, "lagged={}", out.lagged);
        assert!(bus.lagged_total() > 0);
    }

    #[test]
    fn subscriber_sees_only_events_after_subscribe() {
        let bus = Bus::new(64);
        bus.publish(Arc::new(Event::new(
            1,
            1,
            EventKind::UserMessage,
            serde_json::json!({}),
        )));
        let mut s = bus.subscribe(None);
        let out = bus.poll(&mut s);
        assert!(out.events.is_empty());
    }
}
