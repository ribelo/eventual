use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, Criterion};
use eventual::{
    eve::Eve,
    event::{Action, Events, IncomingEvent, Transaction},
};

#[derive(Debug, Default, Clone)]
struct GlobalState {
    count: i32,
}

struct Ping;

impl From<Ping> for IncomingEvent<GlobalState> {
    fn from(event: Ping) -> Self {
        IncomingEvent::Transaction(Box::new(event))
    }
}

#[async_trait]
impl Transaction<GlobalState> for Ping {
    async fn handle(&self, state: &mut GlobalState) -> Events<GlobalState> {
        state.count += 1;
        Events::none()
    }
}

fn fibonacci(n: u32) -> u32 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

struct Fibonacci(u32);

impl From<Fibonacci> for IncomingEvent<GlobalState> {
    fn from(event: Fibonacci) -> Self {
        IncomingEvent::Transaction(Box::new(event))
    }
}

#[async_trait]
impl Transaction<GlobalState> for Fibonacci {
    async fn handle(&self, _state: &mut GlobalState) -> Events<GlobalState> {
        fibonacci(self.0);
        Events::none()
    }
}

fn benchmarks(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut eve = None;
    runtime.block_on(async {
        eve = Some(Eve::new(GlobalState::default()));
    });
    let eve = std::mem::take(&mut eve).unwrap();
    c.bench_function("eve send ping 1e5", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for _ in 0..100_000 {
                    eve.dispatch(Ping).await;
                }
            })
        })
    });
    c.bench_function("eve calc fibo 10x40", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let eve = Eve::new(GlobalState::default());
                for _ in 0..1 {
                    eve.dispatch(Fibonacci(40)).await;
                }
            })
        })
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
