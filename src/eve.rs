pub trait Eve {
    type State: Send + Sync + Default + 'static;
}
