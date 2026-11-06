//! A jq worker for programs that do not run one from their own executable,
//! and for tests. wows-query starts it; it is not run by hand.

fn main() {
    wows_query::jq_worker::init();

    eprintln!("wows-query-jq-worker is started by wows-query to run jq filters");
    std::process::exit(2);
}
