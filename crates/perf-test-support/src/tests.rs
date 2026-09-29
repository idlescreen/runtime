// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn counts_a_real_allocation() {
    let (_, n) = count_allocs(|| {
        let v: Vec<u8> = Vec::with_capacity(64);
        std::hint::black_box(&v);
    });
    assert!(n >= 1, "a Vec::with_capacity must be counted, saw {n}");
}

#[test]
fn sees_nothing_when_idle() {
    let (_, n) = count_allocs(|| {
        let x = 1u64.wrapping_mul(3);
        std::hint::black_box(x);
    });
    assert_eq!(n, 0, "arithmetic on a stack value must not allocate");
}

#[test]
fn a_passing_assertion_does_not_panic() {
    let got = assert_no_alloc("arithmetic", || 2 + 2);
    assert_eq!(got, 4);
}

#[test]
#[should_panic(expected = "allocated")]
fn a_failing_allocation_is_caught() {
    assert_no_alloc("this must allocate", || {
        let v: Vec<u8> = Vec::with_capacity(32);
        std::hint::black_box(&v);
    });
}
