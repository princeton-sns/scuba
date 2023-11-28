use core::Core;
use criterion::{criterion_group, criterion_main, Criterion};

fn setup(turn_off_encryption: bool) {
    static KB: u64 = 1024;
    plaintext_sizes = vec![KB]; //, 2*KB, 4*KB, 8*KB, 16*KB];

    // client_a =
}

pub fn send_unencrypted(c: &mut Criterion) {
    setup(true);
    c.bench_function();
}

pub fn send_encrypted(c: &mut Criterion) {
    setup(false);
    c.bench_function();
}

criterion_group!(benches, send_unencrypted, send_encrypted);
criterion_main!(benches);
