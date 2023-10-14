use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, Criterion};
use eventual::{
    effect::{Event, Events, Transaction},
    eve::Eve,
};

#[derive(Debug, Default)]
struct GlobalState {
    count: i32,
}

struct Ping;

impl From<Ping> for Event<GlobalState> {
    fn from(event: Ping) -> Self {
        Event::Transaction(Box::new(event))
    }
}

#[async_trait]
impl Transaction<GlobalState> for Ping {
    fn handle(&self, state: &mut GlobalState) -> Option<Events<GlobalState>> {
        state.count += 1;
        None
    }
}

fn benchmarks(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("eve send ping 1e5", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let eve = Eve::new(GlobalState::default());
                for _ in 0..100_000 {
                    eve.dispatch(Ping).await;
                }
            })
        })
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
