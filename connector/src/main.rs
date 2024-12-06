use tokio::runtime::Runtime;

mod environment;
mod encryption;
mod connection;

#[tokio::main]
async fn main() {
    let env = match environment::Environment::new() {
        Some(val) => val,
        None => return
    };

    env_logger::builder().filter_level(env.log_level).init();

    let rt = Runtime::new().unwrap();

    for index in 0..env.connections {
        let env = env.clone();
        rt.spawn(async move {
            loop {
                let mut conn: connection::Connection = connection::Connection::new(index, env.clone());
                let _ = conn.start().await;
            }
        });
    }

    std::thread::park();
}
