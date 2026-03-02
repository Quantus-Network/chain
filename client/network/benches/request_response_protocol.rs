// This file is part of Substrate.
// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0
//
// This bench is disabled in the Quantus fork (libp2p-only; no Litep2p,
// no substrate_test_runtime_client). Stub so that `cargo bench` compiles.

use criterion::{criterion_group, criterion_main, Criterion};

fn empty_bench(_: &mut Criterion) {}

criterion_group!(benches, empty_bench);
criterion_main!(benches);
