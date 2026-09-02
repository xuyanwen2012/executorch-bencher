use executorch_bencher::{config::Config, db, http};

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap_or_else(|err| {
        eprintln!("configuration error: {err}");
        std::process::exit(1);
    });

    config.prepare_storage_roots().unwrap_or_else(|err| {
        eprintln!("storage configuration error: {err}");
        std::process::exit(1);
    });

    config.validate_dashboard_dist().unwrap_or_else(|err| {
        eprintln!("dashboard configuration error: {err}");
        std::process::exit(1);
    });

    let pool = db::connect_and_migrate(&config.database_url)
        .await
        .unwrap_or_else(|err| {
            eprintln!("database error: {err}");
            std::process::exit(1);
        });

    let app = http::router(pool, config.clone());

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!("failed to bind listener: {err}");
            std::process::exit(1);
        });

    println!(
        "listening on {}",
        listener
            .local_addr()
            .expect("bound listener has a local address")
    );

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("server error: {err}");
        std::process::exit(1);
    }
}
