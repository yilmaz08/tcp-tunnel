mod environment;
mod encryption;
mod connection;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env: environment::Environment = environment::Environment::new();

    let mut conn: connection::Connection = connection::Connection::new(env.clone());
    return conn.start().await;
}
