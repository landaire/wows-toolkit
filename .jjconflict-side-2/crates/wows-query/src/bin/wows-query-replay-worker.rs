//! A replay worker for programs that do not run one from their own
//! executable, and for tests. wows-query starts it; it is not run by hand.

fn main() {
    wows_query::replay_worker::init();

    eprintln!("wows-query-replay-worker is started by wows-query to read replays");
    std::process::exit(2);
}
