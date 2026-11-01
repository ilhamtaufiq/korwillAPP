mod auth;
mod dashboard;
mod error;
mod export;
mod import;
mod members;
mod users;
mod pdf;
mod photos;
mod projects;
mod riwayat;

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};
use sqlx::{mysql::MySqlPoolOptions, MySqlPool};
use tower_http::services::ServeDir;
use tower_sessions::{MemoryStore, SessionManagerLayer};

#[derive(Clone)]
pub struct AppState {
    pub db: MySqlPool,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/me", get(auth::me))
        .route("/api/me/password", post(auth::change_password))
        .route("/api/dashboard", get(dashboard::get))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route(
            "/api/projects/:id",
            get(projects::get).put(projects::update).delete(projects::delete),
        )
        .route(
            "/api/projects/:id/members",
            get(members::list).post(projects::add_member),
        )
        .route("/api/projects/:id/members/:user_id", delete(members::remove))
        .route("/api/users/tfl", get(members::list_tfl))
        .route(
            "/api/projects/:id/beneficiaries",
            get(projects::list_beneficiaries).post(projects::create_beneficiary),
        )
        .route(
            "/api/projects/:id/beneficiaries/import",
            post(projects::import_beneficiaries),
        )
        .route(
            "/api/beneficiaries/:id",
            put(projects::update_beneficiary).delete(projects::delete_beneficiary),
        )
        .route("/api/projects/import-xlsx", post(import::import_xlsx))
        .route("/api/projects/:id/photos", get(photos::list))
        .route("/api/projects/:id/export.pdf", get(export::export_pdf))
        .route(
            "/api/beneficiaries/:id/photos/:building/:stage",
            post(photos::upload).delete(photos::delete),
        )
        .route("/api/photos/:id/thumb", get(photos::thumb))
        .route("/api/photos/:id/image", get(photos::image))
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/:id", put(users::update))
        .route("/api/users/:id/password", post(users::reset_password))
        .route("/api/log", get(riwayat::list))
        .route("/api/restore/:entitas/:id", post(riwayat::restore))
        // Foto dari HP bisa 5-10 MB, jadi batas default 2 MB terlalu kecil.
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        // Halaman statis (UI) dari folder static/. Dijalankan dari root proyek.
        .fallback_service(ServeDir::new("static"))
        // Sesi disimpan di memori: sesi hilang saat server restart (cukup untuk MVP).
        .layer(SessionManagerLayer::new(MemoryStore::default()).with_secure(false))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "mysql://root@127.0.0.1:3306/sanimas_dev".to_string());
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("create-user") {
        return auth::create_user_cli(&pool, &args[2..]).await;
    }

    let bind = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!("Mendengarkan di http://{bind}");
    axum::serve(listener, router(AppState { db: pool })).await?;
    Ok(())
}
