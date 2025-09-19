use std::fmt::Debug;
use std::mem::{replace, swap};

pub struct RingBuffer<A> {
    buffer: Vec<A>,
    start: usize,
    end: usize,
    capacity: usize,
}

pub struct RingBufferView<'a, A> {
    ring_buffer: &'a RingBuffer<A>,
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

pub fn freeze<'a, A>(ring_buffer: &'a RingBuffer<A>) -> RingBufferView<'a, A> {
    RingBufferView { ring_buffer }
}

impl<'a, A> RingBufferView<'a, A> {
    pub fn at(&'a self, idx: usize) -> Option<&'a A> {
        if idx >= self.ring_buffer.len() {
            return None;
        }
        let idx = (self.ring_buffer.start + idx) % self.ring_buffer.capacity;
        if idx >= self.ring_buffer.buffer.capacity() {
            return None;
        }
        return Some(&self.ring_buffer.buffer[idx]);
    }

    pub fn iter_limited(&'a self, limit: usize) -> RingBufferViewIter<'a, A> {
        RingBufferViewIter {
            buffer: &self.ring_buffer,
            index: 0,
            len: self.ring_buffer.len(),
            limit: Some(limit),
        }
    }

    pub fn len(&self) -> usize {
        self.ring_buffer.len()
    }
}

impl<'a, A> IntoIterator for &'a RingBufferView<'a, A> {
    type Item = &'a A;
    type IntoIter = RingBufferViewIter<'a, A>;

    fn into_iter(self) -> Self::IntoIter {
        RingBufferViewIter {
            buffer: &self.ring_buffer,
            index: 0,
            len: self.ring_buffer.len(),
            limit: None,
        }
    }
}

pub struct RingBufferViewIter<'a, A> {
    buffer: &'a RingBuffer<A>,
    index: usize,
    len: usize,
    limit: Option<usize>,
}

impl<'a, A> Iterator for RingBufferViewIter<'a, A> {
    type Item = &'a A;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.len || self.limit.map_or(false, |l| self.index >= l) {
            return None;
        }
        let idx = (self.buffer.start + self.index) % self.buffer.capacity;
        self.index += 1;
        self.buffer.buffer.get(idx)
    }
}

impl<A> RingBuffer<A> {
    pub fn len(&self) -> usize {
        if self.start == 0 && self.end == 0 {
            return 0;
        } else if self.start < self.end {
            return self.end - self.start;
        } else {
            return self.capacity + self.end - self.start;
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
                if self.end >= self.buffer.len() {
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
            if self.buffer.len() < self.capacity {
                self.buffer.push(val);
            } else {
                if self.end >= self.capacity {
                    self.end = 0
                };
                self.buffer[self.end] = val;
            }
            self.end = (self.end + 1) % self.capacity;
            return None;
        }
    }

    pub fn get_capacity(&self) -> usize {
        self.capacity
    }

    pub fn replace(&mut self, idx: usize, val: A) -> Option<A> {
        if idx < self.len() {
            let dest_idx = (self.start + idx) % self.capacity;
            Some(replace(&mut self.buffer[dest_idx], val))
        } else {
            None
        }
    }

    pub fn insert_at(&mut self, idx: usize, val: A) -> Option<A> {
        let len = self.len();
        if idx > len {
            return Some(val);
        }
        if idx == len {
            return self.push(val);
        }
        let mut val = val;
        let mut write_idx = (self.start + idx) % self.capacity;
        let mut count = len - idx;
        while {
            val = replace(&mut self.buffer[write_idx], val);
            write_idx = (write_idx + 1) % self.capacity;
            count -= 1;
            count > 0
        } {}
        if self.start == 0 && self.end < self.capacity {
            self.end += 1;
        } else {
            self.end = (self.end + 1) % self.capacity;
        }
        if len == self.capacity {
            // ring was full, so we must acknowledge that we overwrote an
            // existing element (which we will return below)
            self.start = (self.start + 1) % self.capacity;
            if self.start == self.end && self.start == 0 {
                self.end = self.capacity;
            }
        }
        if write_idx >= self.buffer.len() {
            self.buffer.push(val);
            return None;
        } else {
            val = replace(&mut self.buffer[write_idx], val);
        }
        if len == self.capacity {
            // ring was already full before inserting, evict last element
            return Some(val);
        } else {
            return None;
        }
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

    pub fn drop_first(&mut self, n: usize) {
        let mut n = n;
        let len = self.len();
        if n >= len {
            self.start = 0;
            self.end = 0;
            return;
        }
        while n > 0 {
            if self.start < self.end {
                self.start += 1;
            } else {
                self.start = (self.start + 1) % self.buffer.capacity();
            }
            n -= 1;
        }
    }

    pub fn with_limited_iter<R, F>(&mut self, limit: usize, f: F) -> R
    where
        F: FnOnce(RingBufferViewIter<'_, A>) -> R,
    {
        return self.with_view(|vw| f(vw.iter_limited(limit)));
    }

    pub fn with_view<R, F>(&mut self, f: F) -> R
    where
        F: FnOnce(RingBufferView<'_, A>) -> R,
    {
        let frozen = freeze(self);
        return f(frozen);
    }
}

impl<A: Debug> RingBuffer<A> {
    pub fn display(&self) {
        print!("buf=");
        for k in 0..self.buffer.len() {
            if k == self.end {
                print!("<");
            }
            if k == self.start {
                print!(">");
            } else {
                print!(" ");
            }
            print!("{:?}", self.buffer[k]);
        }
        if self.end == self.buffer.len() {
            print!("<");
        }
        println!(".");
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
    fn ringbuffer_replace_works() {
        let mut rb = new::<i32>(4);
        rb.push(0);
        rb.push(1);
        rb.push(2); // <0 1 2>
        assert_eq!(rb.replace(3, 99), None);
        assert_eq!(rb.replace(4, 99), None);
        assert_eq!(rb.replace(99, 99), None);
        assert_eq!(rb.replace(0, 10), Some(0));
        assert_eq!(rb.replace(0, 100), Some(10));
        assert_eq!(rb.replace(1, 11), Some(1));
        assert_eq!(rb.replace(1, 101), Some(11));
        assert_eq!(rb.replace(2, 12), Some(2));
        assert_eq!(rb.replace(2, 102), Some(12));
        rb.push(3); // <100 101 102 3>
        rb.push(4); // 4> <101 102 3
        assert_eq!(rb.replace(4, 99), None);
        assert_eq!(rb.replace(99, 99), None);
        assert_eq!(rb.replace(0, 1001), Some(101));
        assert_eq!(rb.replace(3, 14), Some(4));
        rb.push(5); // 14 5> <102 3
        assert_eq!(rb.replace(3, 15), Some(5));
        assert_eq!(rb.replace(2, 104), Some(14));
        assert_eq!(rb.replace(1, 13), Some(3));
        assert_eq!(rb.replace(0, 1002), Some(102));
        rb.drop_first(1); // 104 15> <13
        assert_eq!(rb.replace(3, 99), None);
        assert_eq!(rb.replace(2, 105), Some(15));
        assert_eq!(rb.replace(1, 1004), Some(104));
        assert_eq!(rb.replace(0, 103), Some(13));
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
        {
            let rbv = freeze(&rb);
            assert_eq!(rbv.at(0), Some(4).as_ref());
            assert_eq!(rbv.at(1), Some(6).as_ref());
            assert_eq!(rbv.at(2), None);
            assert_eq!(rbv.at(3), None);
        }
        rb.halve_data();
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get_capacity(), 7);
        {
            let rbv = freeze(&rb);
            assert_eq!(rbv.at(0), Some(6).as_ref());
            assert_eq!(rbv.at(1), None);
            assert_eq!(rbv.at(2), None);
            assert_eq!(rbv.at(3), None);
        }

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
        {
            let rbv = freeze(&rb);
            assert_eq!(rbv.at(0), Some(4).as_ref());
            assert_eq!(rbv.at(1), Some(6).as_ref());
            assert_eq!(rbv.at(2), None);
            assert_eq!(rbv.at(3), None);
        }
        rb.halve_data();
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get_capacity(), 4);
        let rbv = freeze(&rb);
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
        let rbv = freeze(&rb);
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
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), Some(8).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rbv.at(5), None);
        assert_eq!(rbv.at(6), None);
        rb.halve_data();
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(&rb);
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
        let rbv = freeze(&rb);
        for i in 0..7 {
            // proof that rb = 1 2 3 4 5 6 7
            assert_eq!(rbv.at(i), Some(i + 1).as_ref());
        }
        rb.halve_data(); // should be 2 4 6
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get_capacity(), 7);
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(2).as_ref());
        assert_eq!(rbv.at(1), Some(4).as_ref());
        assert_eq!(rbv.at(2), Some(6).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);

        let mut rb = new::<usize>(8);
        for i in 0..15 {
            rb.push(i);
        }
        let rbv = freeze(&rb);
        for i in 0..8 {
            // proof that rb = 7 8 9 10 11 12 13 14
            assert_eq!(rbv.at(i), Some(i + 7).as_ref());
        }
        rb.halve_data(); // should be 8 10 12 14
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.get_capacity(), 8);
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(8).as_ref());
        assert_eq!(rbv.at(1), Some(10).as_ref());
        assert_eq!(rbv.at(2), Some(12).as_ref());
        assert_eq!(rbv.at(3), Some(14).as_ref());
        assert_eq!(rbv.at(4), None);
    }

    #[test]
    fn ringbuffer_freeze_and_thaw_overwrites_when_pushing_enough() {
        let mut rb = new::<i32>(3);
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), None);
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rbv.at(5), None);
        assert_eq!(rb.push(3), None);
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(3).as_ref());
        assert_eq!(rbv.at(1), None);
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rbv.at(5), None);
        assert_eq!(rb.push(4), None);
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(3).as_ref());
        assert_eq!(rbv.at(1), Some(4).as_ref());
        assert_eq!(rbv.at(2), None);
        assert_eq!(rbv.at(3), None);
        assert_eq!(rb.push(5), None);
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(3).as_ref());
        assert_eq!(rbv.at(1), Some(4).as_ref());
        assert_eq!(rbv.at(2), Some(5).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rb.push(6), Some(3));
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(4).as_ref());
        assert_eq!(rbv.at(1), Some(5).as_ref());
        assert_eq!(rbv.at(2), Some(6).as_ref());
        assert_eq!(rbv.at(3), None);
        assert_eq!(rbv.at(4), None);
        assert_eq!(rb.push(7), Some(4));
        let rbv = freeze(&rb);
        assert_eq!(rbv.at(0), Some(5).as_ref());
        assert_eq!(rbv.at(1), Some(6).as_ref());
        assert_eq!(rbv.at(2), Some(7).as_ref());
        assert_eq!(rbv.at(3), None);
    }

    #[test]
    pub fn test_drop_first() {
        let mut val = 8;
        let mut extra_len = 0;
        let mut rb = new::<usize>(10);
        rb.drop_first(5);
        assert_eq!(rb.len(), 0);
        rb.drop_first(5);
        assert_eq!(rb.len(), 0);
        for j in 3..8 {
            let mut k = j;
            while k > 0 {
                rb.push(val);
                let now_len = rb.len();
                let rbv = freeze(&rb);
                assert_eq!(rbv.at(now_len), None);
                k -= 1;
                val += 1;
            }
            assert_eq!(rb.len(), j + extra_len);
            let now_len = rb.len();
            let rbv = freeze(&rb);
            let mut expected_val = val - 1;
            assert_eq!(rbv.at(now_len), None);
            k = now_len;
            while k > 0 {
                k -= 1;
                assert_eq!(rbv.at(k), Some(expected_val).as_ref());
                expected_val -= 1;
            }
            rb.drop_first(2);
            assert_eq!(rb.len(), j - 2 + extra_len);
            let now_len = rb.len();
            let rbv = freeze(&rb);
            assert_eq!(rbv.at(now_len), None);
            let mut expected_val = val - 1;
            assert_eq!(rbv.at(now_len), None);
            k = now_len;
            while k > 0 {
                k -= 1;
                assert_eq!(rbv.at(k), Some(expected_val).as_ref());
                expected_val -= 1;
            }
            rb.drop_first(1);
            assert_eq!(rb.len(), j - 3 + extra_len);
            extra_len = rb.len();
            if (extra_len + j + 1) >= rb.get_capacity() {
                rb.drop_first(extra_len);
                extra_len = 0;
            }
        }
    }

    #[test]
    fn test_ring_buffer_iter_all() {
        let mut rb = new(5);
        for i in 0..5 {
            rb.push(i);
        }
        let view = freeze(&rb);
        let collected: Vec<_> = view.into_iter().cloned().collect();
        assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_ring_buffer_iter_limited() {
        let mut rb = new(5);
        for i in 0..5 {
            rb.push(i);
        }
        let view = freeze(&rb);
        let collected: Vec<_> = view.iter_limited(3).cloned().collect();
        assert_eq!(collected, vec![0, 1, 2]);
    }

    #[test]
    fn test_ring_buffer_wraparound() {
        let mut rb = new(5);
        for i in 0..7 {
            // cause wraparound by inserting more data than can fit
            rb.push(i);
        }
        let view = freeze(&rb);
        let collected: Vec<_> = view.into_iter().cloned().collect();
        assert_eq!(collected, vec![2, 3, 4, 5, 6]);
        rb.drop_first(2);
        let view = freeze(&rb);
        let collected: Vec<_> = view.into_iter().cloned().collect();
        assert_eq!(collected, vec![4, 5, 6]);
        let collected: Vec<_> = view.iter_limited(3).cloned().collect();
        assert_eq!(collected, vec![4, 5, 6]);
        let collected: Vec<_> = view.iter_limited(2).cloned().collect();
        assert_eq!(collected, vec![4, 5]);
        rb.push(7);
        let view = freeze(&rb);
        let collected: Vec<_> = view.into_iter().cloned().collect();
        assert_eq!(collected, vec![4, 5, 6, 7]);
        let collected: Vec<_> = view.iter_limited(3).cloned().collect();
        assert_eq!(collected, vec![4, 5, 6]);
        let collected: Vec<_> = view.iter_limited(20).cloned().collect();
        assert_eq!(collected, vec![4, 5, 6, 7]);
    }

    #[test]
    fn test_ring_buffer_iter_empty() {
        let rb = new::<i32>(5);
        let view = freeze(&rb);
        let collected: Vec<_> = view.into_iter().cloned().collect();
        assert!(collected.is_empty());
    }

    #[test]
    fn test_ring_buffer_insert_at() {
        let mut rb = new::<&'static str>(8);
        assert_eq!(rb.insert_at(1, "ignored"), Some("ignored"));
        assert_eq!(rb.insert_at(99, "ignored,99"), Some("ignored,99"));
        assert_eq!(rb.insert_at(0, "a"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a"]
        );
        rb.push("b");
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(rb.insert_at(0, "a0"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a0", "a", "b"]
        );
        assert_eq!(rb.insert_at(1, "a1"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a0", "a1", "a", "b"]
        );
        assert_eq!(rb.insert_at(3, "b1"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a0", "a1", "a", "b1", "b"]
        );
        assert_eq!(rb.insert_at(3, "b0"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a0", "a1", "a", "b0", "b1", "b"]
        );
        assert_eq!(rb.insert_at(5, "b2"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a0", "a1", "a", "b0", "b1", "b2", "b"]
        );
        assert_eq!(rb.insert_at(6, "b3"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a0", "a1", "a", "b0", "b1", "b2", "b3", "b"]
        );
        assert_eq!(rb.insert_at(7, "b4"), Some("a0"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a1", "a", "b0", "b1", "b2", "b3", "b4", "b"]
        );
        assert_eq!(rb.insert_at(8, "c"), Some("a1"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a", "b0", "b1", "b2", "b3", "b4", "b", "c"]
        );
        assert_eq!(rb.insert_at(1, "d"), Some("a"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["d", "b0", "b1", "b2", "b3", "b4", "b", "c"]
        );
        rb.drop_first(3);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b2", "b3", "b4", "b", "c"]
        );
        assert_eq!(rb.insert_at(1, "e"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b2", "e", "b3", "b4", "b", "c"]
        );
        assert_eq!(rb.insert_at(3, "f"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b2", "e", "b3", "f", "b4", "b", "c"]
        );
        assert_eq!(rb.insert_at(1, "b5"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b2", "b5", "e", "b3", "f", "b4", "b", "c"]
        );
        assert_eq!(rb.insert_at(2, "b6"), Some("b2"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b5", "b6", "e", "b3", "f", "b4", "b", "c"]
        );
        assert_eq!(rb.insert_at(8, "h"), Some("b5"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b6", "e", "b3", "f", "b4", "b", "c", "h"]
        );
        assert_eq!(rb.insert_at(7, "g"), Some("b6"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["e", "b3", "f", "b4", "b", "c", "g", "h"]
        );
        assert_eq!(rb.insert_at(6, "d"), Some("e"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b3", "f", "b4", "b", "c", "d", "g", "h"]
        );
        assert_eq!(rb.insert_at(6, "e"), Some("b3"));
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["f", "b4", "b", "c", "d", "e", "g", "h"]
        );
        rb.drop_first(2);
        assert_eq!(rb.insert_at(4, "f"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["b", "c", "d", "e", "f", "g", "h"]
        );
        assert_eq!(rb.insert_at(0, "a"), None);
        assert_eq!(
            freeze(&rb).into_iter().cloned().collect::<Vec<_>>(),
            vec!["a", "b", "c", "d", "e", "f", "g", "h"]
        );
    }
}
