use std::mem::swap;

pub struct RingBuffer<A> {
    buffer: Vec<A>,
    start: usize,
    end: usize,
    capacity: usize,
}

// impl<A> Default for RingBuffer<A> {
//     fn default() -> Self { new::<A>(10) }
// }

pub struct RingBufferView<A> {
    ring_buffer: RingBuffer<A>,
}

pub fn new<A>(size: usize) -> RingBuffer<A> {
    assert!(size > 0);
    RingBuffer {
        buffer: Vec::<A>::with_capacity(size),
        start: 0,
        end: 0,
        capacity: size,
    }
}

pub fn freeze<A>(ring_buffer: RingBuffer<A>) -> RingBufferView<A> {
    RingBufferView { ring_buffer }
}

impl<A> RingBufferView<A> {
    pub fn at<'a>(&'a self, idx: usize) -> Option<&'a A> {
        if idx >= self.ring_buffer.len() {
            return None;
        }
        let idx = (self.ring_buffer.start + idx) % self.ring_buffer.capacity;
        if idx >= self.ring_buffer.buffer.capacity() {
            return None;
        }
        return Some(&self.ring_buffer.buffer[idx]);
    }

    pub fn thaw(self) -> RingBuffer<A> {
        self.ring_buffer
    }
}

impl<A> RingBuffer<A> {
    pub fn len(&self) -> usize {
        if self.start == 0 && self.end == 0 {
            return 0;
        } else if self.start < self.end {
            return self.end - self.start;
        } else {
            return self.buffer.len() + self.end - self.start;
        }
    }

    pub fn peek_first<B>(&self, cont: fn(&A) -> B) -> Option<B> {
        if self.start == 0 && self.end == 0 {
            return None;
        } else {
            return Some(cont(&self.buffer[self.start]));
        }
    }

    pub fn peek_last<B>(&self, cont: fn(&A) -> B) -> Option<B> {
        if self.start == 0 && self.end == 0 {
            return None;
        } else {
            return Some(cont(&self.buffer[self.end - 1]));
        }
    }

    pub fn push(&mut self, val: A) -> Option<A> {
        if self.start == 0 {
            if self.end >= self.capacity {
                let mut val = val;
                swap(&mut self.buffer[0], &mut val);
                self.start = 1;
                self.end = 1;
                return Some(val);
            } else {
                if self.buffer.len() < self.buffer.capacity() {
                    self.buffer.push(val);
                } else {
                    self.buffer[self.end] = val;
                }
                self.end += 1;
                return None;
            }
        } else if self.start == self.end {
            let mut val = val;
            swap(&mut self.buffer[self.end], &mut val);
            self.end += 1;
            if self.end < self.capacity {
                self.start = self.end;
            } else {
                self.start = 0;
            }
            return Some(val);
        } else {
            self.buffer[self.end] = val;
            self.end += 1;
            return None;
        }
    }

    pub fn get_capacity(&self) -> usize {
        self.capacity
    }

    pub fn halve_data(&mut self) {
        let len = self.len();
        if len <= 1 {
            self.start = 0;
            self.end = 0;
            return;
        }
        let new_len = len / 2;
        let mut read_idx = (self.start + 1) % self.capacity;
        let mut write_idx = self.start;
        for _ in 0..new_len {
            self.buffer.swap(read_idx, write_idx);
            read_idx = (read_idx + 2) % self.capacity;
            write_idx = (write_idx + 1) % self.capacity;
        }
        self.end = write_idx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_ringbuffer_len_is_0() {
        assert_eq!(new::<&str>(5).len(), 0);
    }

    #[test]
    fn ringbuffer_capacity_is_correct() {
        assert_eq!(new::<&str>(5).get_capacity(), 5);
    }

    fn idint(x: &i32) -> i32 {
        *x
    }

    fn strlen(x: &&str) -> usize {
        x.len()
    }

    #[test]
    fn fresh_ringbuffer_peek_is_none() {
        assert_eq!(new::<&str>(23).peek_first(strlen), None);
        assert_eq!(new::<i32>(3).peek_last(idint), None);
    }

    #[test]
    fn fresh_ringbuffer_peek_when_filling() {
        let mut rb = new::<i32>(3);
        rb.push(3);
        assert_eq!(rb.peek_first(idint), Some(3));
        assert_eq!(rb.peek_last(idint), Some(3));
        rb.push(4);
        assert_eq!(rb.peek_first(idint), Some(3));
        assert_eq!(rb.peek_last(idint), Some(4));
        rb.push(5);
        assert_eq!(rb.peek_first(idint), Some(3));
        assert_eq!(rb.peek_last(idint), Some(5));
        rb.push(6);
        assert_eq!(rb.peek_first(idint), Some(4));
        assert_eq!(rb.peek_last(idint), Some(6));
        rb.push(7);
        assert_eq!(rb.peek_first(idint), Some(5));
        assert_eq!(rb.peek_last(idint), Some(7));
        rb.push(8);
        assert_eq!(rb.peek_first(idint), Some(6));
        assert_eq!(rb.peek_last(idint), Some(8));
        rb.push(9);
        assert_eq!(rb.peek_first(idint), Some(7));
        assert_eq!(rb.peek_last(idint), Some(9));
    }

    #[test]
    fn ringbuffer_overwrites_when_pushing_enough() {
        let mut rb = new::<i32>(3);
        assert_eq!(rb.push(3), None);
        assert_eq!(rb.push(4), None);
        assert_eq!(rb.push(5), None);
        assert_eq!(rb.push(6), Some(3));
        assert_eq!(rb.push(7), Some(4));
    }

    #[test]
    fn ringbuffer_len_keeps_working_even_when_pushing_enough() {
        let mut rb = new::<i32>(3);
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.push(3), None);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.push(4), None);
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.push(5), None);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.push(6), Some(3));
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.push(7), Some(4));
        assert_eq!(rb.len(), 3);
    }

    #[test]
    fn ringbuffer_halve_data_even_length() {
        let mut rb = new::<i32>(7);
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.push(3), None);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.push(4), None);
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.push(5), None);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.push(6), None);
        assert_eq!(rb.len(), 4);
        rb.halve_data();
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        let mut rb = rbv.thaw();
        rb.halve_data();
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(6).as_ref());
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);

        let mut rb = new::<i32>(4);
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.push(3), None);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.push(4), None);
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.push(5), None);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.push(6), None);
        assert_eq!(rb.len(), 4);
        rb.halve_data();
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.get_capacity(), 4);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        let mut rb = rbv.thaw();
        rb.halve_data();
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get_capacity(), 4);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(6).as_ref());
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
    }

    #[test]
    fn ringbuffer_halve_data_odd_length() {
        let mut rb = new::<i32>(7);
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.push(3), None);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.push(4), None);
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.push(5), None);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.push(6), None);
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.push(7), None);
        assert_eq!(rb.len(), 5);
        rb.halve_data();
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);

        let mut rb = new::<i32>(7);
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.push(3), None);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.push(4), None);
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.push(5), None);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.push(6), None);
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.push(7), None);
        assert_eq!(rb.len(), 5);
        assert_eq!(rb.push(8), None);
        assert_eq!(rb.len(), 6);
        assert_eq!(rb.push(9), None);
        assert_eq!(rb.len(), 7);
        rb.halve_data();
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), Some(8).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rbv.at(5), None);
        assert_eq!(rbv.at(6), None);
        let mut rb = rbv.thaw();
        rb.halve_data();
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(6).as_ref());
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
    }

    #[test]
    fn ringbuffer_halve_data_after_wrap_around() {
        let mut rb = new::<usize>(7);
        for i in 0..8 {
            rb.push(i);
        }
        let rbv = freeze(rb);
        for i in 0..7 { // proof that rb = 1 2 3 4 5 6 7
            assert_eq!(rbv.at(i), Some(i + 1).as_ref());
        }
        let mut rb = rbv.thaw();
        rb.halve_data(); // should be 2 4 6
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(2).as_ref());
        assert_eq!(rbv.at(1), Some(4).as_ref());
        assert_eq!(rbv.at(2), Some(6).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);

        let mut rb = new::<usize>(8);
        for i in 0..15 {
            rb.push(i);
        }
        let rbv = freeze(rb);
        for i in 0..8 { // proof that rb = 7 8 9 10 11 12 13 14
            assert_eq!(rbv.at(i), Some(i + 7).as_ref());
        }
        let mut rb = rbv.thaw();
        rb.halve_data(); // should be 8 10 12 14
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.get_capacity(), 8);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(8).as_ref());
        assert_eq!(rbv.at(1), Some(10).as_ref());
        assert_eq!(rbv.at(2), Some(12).as_ref());
        assert_eq!(rbv.at(3), Some(14).as_ref());
        assert_eq!(rbv.at(4), None);
    }

    #[test]
    fn ringbuffer_freeze_and_thaw_overwrites_when_pushing_enough() {
        let rbv = freeze(new::<i32>(3));
        assert_eq!(rbv.at(0), None);
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rbv.at(5), None);
        let mut rb = rbv.thaw();
        assert_eq!(rb.push(3), None);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(3).as_ref());
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rbv.at(5), None);
        let mut rb = rbv.thaw();
        assert_eq!(rb.push(4), None);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(3).as_ref());
        assert_eq!(rbv.at(1), Some(4).as_ref());
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        let mut rb = rbv.thaw();
        assert_eq!(rb.push(5), None);
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(3).as_ref());
        assert_eq!(rbv.at(1), Some(4).as_ref());
        assert_eq!(rbv.at(2), Some(5).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        let mut rb = rbv.thaw();
        assert_eq!(rb.push(6), Some(3));
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(5).as_ref());
        assert_eq!(rbv.at(2), Some(6).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        let mut rb = rbv.thaw();
        assert_eq!(rb.push(7), Some(4));
        let rbv = freeze(rb);
        assert_eq!(rbv.at(0), Some(5).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), Some(7).as_ref());
        assert_eq!(rbv.at(3), None);
    }
}
