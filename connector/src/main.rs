use tokio::runtime::Runtime;

mod environment;
mod encryption;
mod connection;

#[tokio::main]
async fn main() {
    let env: environment::Environment = environment::Environment::new();

    let rt = Runtime::new().unwrap();

    for index in 0..env.connections {
        let env = env.clone();
        rt.spawn(async move {
            loop {
                let mut conn: connection::Connection = connection::Connection::new(index, env.clone());
                let _ = conn.start().await;
                println!("#{:?} Ended", index);
            }
        });
    }

    std::thread::park();
}
