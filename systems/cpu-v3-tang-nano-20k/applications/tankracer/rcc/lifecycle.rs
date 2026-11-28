//! Live tick cursor and deferred LIFO queues. The world calls methods directly.
use crate::dsl_rt::*;
use crate::{attitude, integer, ordered};
pub struct Ticks {
    pub flags: Array<u16>,
    // One word for checked u16 flags, two for the original raw i32 image.
    pub flag_words: u16,
    pub flag_count: u16,
    pub order: ordered::List,
    pub next: fix32,
}
pub struct Queues {
    pub notifications: ordered::List,
    pub destruction: ordered::List,
}
pub struct Pass {
    pub current: fix32,
    pub position: u16,
    pub work: fix32,
    pub status: u16,
}
pub struct Drain {
    pub destroy: bool,
    pub work: fix32,
    pub status: u16,
}
pub struct Event {
    pub status: u16,
    pub id: fix32,
    pub destroy: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn valid(t: &Ticks, id: fix32) -> bool {
    id.hi_bits() == 0 && id.lo_bits() < t.flag_count
}
pub fn flags(t: &Ticks, id: u16) -> fix32 {
    if t.flag_words == 1 {
        return fix32::from_words(t.flags[id], 0);
    }
    attitude::get(t.flags, id)
}
pub fn clear(t: &Ticks, id: u16) {
    let mut words = t.flags;
    words[id * t.flag_words] = 0;
    if t.flag_words == 2 {
        words[id * 2 + 1] = 0;
    }
}
fn exhausted(work: fix32, limit: fix32) -> bool {
    work.hi_bits() > limit.hi_bits()
        || (work.hi_bits() == limit.hi_bits() && work.lo_bits() >= limit.lo_bits())
}
// Mutation status: 0 success/unchanged, 1 changed, 2 invalid limit,
// 3 work limit, 4 capacity, 5 invalid cursor, 6 missing entity.
pub fn enable(t: &mut Ticks, id: fix32) -> u16 {
    if !valid(t, id) {
        return 6;
    }
    let index = id.lo_bits() * t.flag_words;
    let mut flags = t.flags;
    if flags[index] & 16 != 0 {
        return 0;
    }
    let status = ordered::append(&mut t.order, id);
    if status != 0 {
        return status;
    }
    flags[index] |= 16;
    1
}
pub fn disable(t: &mut Ticks, id: fix32, limit: fix32) -> u16 {
    if limit == n(0) {
        return 2;
    }
    if !valid(t, id) {
        return 6;
    }
    let index = id.lo_bits() * t.flag_words;
    let mut flags = t.flags;
    if flags[index] & 16 == 0 {
        return 0;
    }
    if t.next == id {
        let found = ordered::find(&t.order, id, fix32::from_words(0, 1));
        if !found.found {
            return 5;
        }
        t.next = n(-1);
        if found.index + 1 < t.order.count {
            t.next = attitude::get(t.order.items, found.index + 1);
        }
    }
    let status = ordered::remove_first(&mut t.order, id, limit);
    if status != 0 {
        return status;
    }
    flags[index] &= 65487;
    1
}
pub fn queue(q: &mut Queues, t: &mut Ticks, id: fix32, destroy: bool, limit: fix32) -> u16 {
    if limit == n(0) {
        return 2;
    }
    if !valid(t, id) {
        return 6;
    }
    let index = id.lo_bits() * t.flag_words;
    let mut flags = t.flags;
    if destroy && flags[index] & 32 != 0 {
        return 0;
    }
    flags[index] |= 32;
    if destroy {
        let found = ordered::find(&q.destruction, id, limit);
        if found.status != 0 {
            return found.status;
        }
        if !found.found {
            return ordered::prepend(&mut q.destruction, id);
        }
        return 0;
    }
    let found = ordered::find(&q.notifications, id, limit);
    if found.status != 0 {
        return found.status;
    }
    if !found.found {
        return ordered::prepend(&mut q.notifications, id);
    }
    0
}
pub fn cancel(q: &mut Queues, t: &mut Ticks, id: fix32, limit: fix32) -> u16 {
    if limit == n(0) {
        return 2;
    }
    if !valid(t, id) {
        return 6;
    }
    let mut flags = t.flags;
    flags[id.lo_bits() * t.flag_words] &= 65503;
    ordered::remove_first(&mut q.notifications, id, limit)
}
pub fn begin(t: &Ticks, limit: fix32) -> Pass {
    let mut pass = Pass {
        current: n(-1),
        position: 0,
        work: n(0),
        status: 0,
    };
    if limit == n(0) {
        pass.status = 2;
        return pass;
    }
    if t.order.count != 0 {
        pass.current = attitude::get(t.order.items, 0);
    }
    pass
}
// Save the successor before dispatch. The caller resumes only after success.
pub fn prepare(t: &mut Ticks, pass: &mut Pass, limit: fix32) -> u16 {
    if pass.status != 0 {
        return pass.status;
    }
    if pass.current == n(-1) {
        t.next = n(-1);
        return 0;
    }
    if exhausted(pass.work, limit) {
        return 3;
    }
    pass.work = pass.work + n(1);
    let mut index = pass.position;
    if index >= t.order.count || attitude::get(t.order.items, index) != pass.current {
        let found = ordered::find(&t.order, pass.current, fix32::from_words(0, 1));
        if !found.found {
            return 5;
        }
        index = found.index;
    }
    pass.position = index + 1;
    t.next = n(-1);
    if pass.position < t.order.count {
        t.next = attitude::get(t.order.items, pass.position);
    }
    1
}
pub fn resume(pass: &mut Pass, t: &Ticks) {
    pass.current = t.next;
}
pub fn begin_drain(limit: fix32) -> Drain {
    let mut drain = Drain {
        destroy: false,
        work: n(0),
        status: 0,
    };
    if limit == n(0) {
        drain.status = 2;
    }
    drain
}
// Both terminal empty pops spend budget. Stop precedes Destroy on each item.
pub fn pop(q: &mut Queues, drain: &mut Drain, limit: fix32) -> Event {
    let mut event = Event {
        status: 0,
        id: n(-1),
        destroy: false,
    };
    if drain.status != 0 {
        event.status = drain.status;
        return event;
    }
    let mut phase = 0u16;
    while phase < 2 {
        if exhausted(drain.work, limit) {
            event.status = 3;
            return event;
        }
        drain.work = drain.work + n(1);
        if drain.destroy {
            if q.destruction.count != 0 {
                event.id = attitude::get(q.destruction.items, 0);
                ordered::remove_at(&mut q.destruction, 0);
                event.status = 1;
                event.destroy = true;
            }
            return event;
        }
        if q.notifications.count != 0 {
            event.id = attitude::get(q.notifications.items, 0);
            ordered::remove_at(&mut q.notifications, 0);
            event.status = 1;
            event.destroy = drain.destroy;
            return event;
        }
        drain.destroy = true;
        phase += 1;
    }
    event
}
