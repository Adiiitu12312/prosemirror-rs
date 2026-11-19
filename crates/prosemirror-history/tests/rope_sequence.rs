//! Port of rope-sequence 1.3.4, `test.js`.

use prosemirror_history::RopeSequence;

const SIZE: usize = 10_000;

fn append_build(size: usize) -> RopeSequence<usize> {
    (0..size).fold(RopeSequence::new(), |rope, value| rope.append(vec![value]))
}

fn deque_build(size: usize) -> RopeSequence<usize> {
    let middle = size / 2;
    let mut rope = RopeSequence::new();
    for (index, upper) in (middle..size).enumerate() {
        rope = rope.append(vec![upper]);
        if let Some(lower) = middle.checked_sub(index + 1) {
            rope = rope.prepend(vec![lower]);
        }
    }
    rope
}

fn check_for_each(rope: &RopeSequence<usize>, from: usize, to: usize, offset: usize) {
    let mut current = from;
    assert!(rope.for_each(from, to, |&value, index| {
        assert_eq!(value, current + offset);
        assert_eq!(index, current);
        current += 1;
        true
    }));
    assert_eq!(current, to);
    assert!(rope.for_each(to, from, |&value, index| {
        current -= 1;
        assert_eq!(value, current + offset);
        assert_eq!(index, current);
        true
    }));
    assert_eq!(current, from);
}

fn choose(seed: &mut u64, limit: usize) -> usize {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (*seed >> 32) as usize % limit
}

fn check(rope: &RopeSequence<usize>, size: usize, offset: usize, seed: &mut u64) {
    assert_eq!(rope.len(), size);
    for index in 0..size {
        assert_eq!(rope.get(index), Some(&(offset + index)));
    }
    assert_eq!(rope.get(size), None);
    check_for_each(rope, 0, size, offset);
    for _ in 0..10.min(size / 100) {
        let start = choose(seed, size);
        let end = start + 1 + choose(seed, size - start);
        check_for_each(rope, start, end, offset);
        if end - start < size {
            check(&rope.slice(start, end), end - start, offset + start, seed);
        }
    }
}

#[test]
fn appending_prepending_and_flat_creation_preserve_order_through_slicing() {
    let mut seed = 1234;
    check(&append_build(SIZE), SIZE, 0, &mut seed);
    check(&deque_build(SIZE), SIZE, 0, &mut seed);
    check(&RopeSequence::from((0..SIZE).collect::<Vec<_>>()), SIZE, 0, &mut seed);
}

#[test]
fn empty_operations_preserve_identity_and_iteration_can_stop_early() {
    let small = RopeSequence::from(vec![1, 2, 4]);
    let empty = RopeSequence::new();
    assert!(small.append(&empty).ptr_eq(&small));
    assert!(small.prepend(&empty).ptr_eq(&small));
    assert!(empty.append(&empty).ptr_eq(&empty));
    assert!(small.slice(0, 0).ptr_eq(&empty));
    assert!(small.slice(0, small.len()).ptr_eq(&small));

    let mut sum = 0;
    assert!(!small.for_each(0, small.len(), |&value, _| {
        if value == 2 {
            return false;
        }
        sum += value;
        true
    }));
    assert_eq!(sum, 1);
    assert_eq!(small.map(0, small.len(), |value, _| value + 1), [2, 3, 5]);
    assert_eq!(small.flatten(), [1, 2, 4]);
}
