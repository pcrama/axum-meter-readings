type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
struct AppState {
    db: HashMap<String, Bytes>,
    counter: i32,
}

impl AppState {
    fn set_counter(&mut self, val: i32) {
        self.counter = val;
    }

    fn get_counter(&self) -> i32 {
        self.counter
    }
}
