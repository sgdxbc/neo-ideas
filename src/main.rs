use axum::{routing::get, Router};
use tokio::spawn;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = Router::new().route("/", get(|| async { "Hello, World!" }));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    let serve_handle = spawn(async move { axum::serve(listener, app).await });
    tokio::select! {
        result = serve_handle => result??,
        result = tokio::signal::ctrl_c() => {
            result?;
            println!()
        }
    }
    Ok(())
}
