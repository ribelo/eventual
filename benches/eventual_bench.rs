use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, Criterion};
use eventual::{effect::Effects, eve::create_eve_handlers, event::Event, side_effect::SideEffect};

#[derive(Debug, Default)]
struct GlobalState {
    count: i32,
}

struct Ping;

#[async_trait]
impl SideEffect<GlobalState> for Ping {
    async fn handle(&self, state: &mut GlobalState) -> Effects<GlobalState> {
        state.count += 1;
        Effects::none()
    }
}

fn benchmarks(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("eve send ping 1e5", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let (eve_event_handler, eve_side_effect_handler) =
                    create_eve_handlers(GlobalState::default());
                // let event_tx = eve_event_handler.event_tx.clone();
                let side_effect_tx = eve_event_handler.side_effext_tx.clone();
                eve_event_handler.spawn_loop().await;
                eve_side_effect_handler.spawn_loop().await;
                for _ in 0..100_000 {
                    side_effect_tx.send(Box::new(Ping)).await.unwrap();
                }
            })
        })
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
