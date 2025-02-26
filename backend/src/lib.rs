/*
 * Author & Copyright: Ruben Fiszel 2021
 * This file and its contents are licensed under the AGPLv3 License.
 * Please see the included NOTICE for copyright information and
 * LICENSE-AGPL for a copy of the license.
 */

use ::oauth2::basic::BasicClient;
use argon2::Argon2;
use axum::{handler::Handler, middleware::from_extractor, routing::get, Extension, Router};
use db::DB;
use git_version::git_version;
use hyper::Response;
use slack_http_verifier::SlackVerifier;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_cookies::CookieManagerLayer;
use tower_http::trace::{MakeSpan, OnResponse, TraceLayer};
use tracing::{field, Span};
use tracing_subscriber::{filter::filter_fn, prelude::*, EnvFilter};
extern crate magic_crypt;

extern crate dotenv;

mod audit;
mod client;
mod db;
mod email;
mod error;
mod flow;
mod granular_acls;
mod groups;
mod jobs;
mod js_eval;
mod oauth2;
mod parser;
mod resources;
mod schedule;
mod scripts;
mod static_assets;
mod users;
mod utils;
mod variables;
mod worker;
mod worker_ping;
mod workspaces;

use error::Error;

pub use crate::email::EmailSender;
use crate::{db::UserDB, utils::rd_string};

const GIT_VERSION: &str = git_version!(args = ["--tag", "--always"], fallback = "unknown-version");
pub const DEFAULT_NUM_WORKERS: usize = 3;
pub const DEFAULT_TIMEOUT: i32 = 300;
pub const DEFAULT_SLEEP_QUEUE: u64 = 50;

#[derive(Clone)]
struct MyOnResponse {}

impl<B> OnResponse<B> for MyOnResponse {
    fn on_response(
        self,
        response: &Response<B>,
        latency: std::time::Duration,
        _span: &tracing::Span,
    ) {
        tracing::info!(
            latency = %latency.as_millis(),
            status = ?response.status(),
            "finished processed request")
    }
}

#[derive(Clone)]
struct MyMakeSpan {}

impl<B> MakeSpan<B> for MyMakeSpan {
    fn make_span(&mut self, request: &hyper::Request<B>) -> Span {
        tracing::info_span!(
            "request",
            method = %request.method(),
            uri = %request.uri(),
            version = ?request.version(),
            username = field::Empty,
        )
    }
}

pub async fn initialize_tracing() -> anyhow::Result<()> {
    //let log_level = if std::env::var("RUST_LOG").map(|x| &x == "debug")
    let ts_base = tracing_subscriber::registry()
        .with(
            EnvFilter::from_default_env()
                //.add_directive("windmill".parse()?)
                .add_directive("runtime=trace".parse()?)
                .add_directive("tokio=trace".parse()?),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_span_list(false)
                .with_current_span(true)
                .with_filter(filter_fn(|meta| meta.target().starts_with("windmill"))),
        );

    if std::env::var("TOKIO_CONSOLE")
        .map(|x| x == "true")
        .unwrap_or(false)
    {
        let console_layer = console_subscriber::spawn();
        ts_base.with(console_layer).init();
    } else {
        ts_base.init();
    }
    Ok(())
}

pub async fn migrate_db(db: &DB) -> anyhow::Result<()> {
    let app_password = std::env::var("APP_USER_PASSWORD").unwrap_or_else(|_| "changeme".to_owned());

    db::migrate(db).await?;
    db::setup_app_user(db, &app_password).await?;
    Ok(())
}

pub async fn connect_db() -> anyhow::Result<DB> {
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| Error::BadConfig("DATABASE_URL env var is missing".to_string()))?;
    Ok(db::connect(&database_url).await?)
}

type BasicClientsMap = HashMap<String, BasicClient>;

pub fn build_oauth_clients(base_url: &str) -> BasicClientsMap {
    [(
        "github".to_string(),
        oauth2::build_gh_client(
            &std::env::var("GITHUB_OAUTH_CLIENT_ID").unwrap_or_else(|_| "".to_string()),
            &std::env::var("GITHUB_OAUTH_CLIENT_SECRET").unwrap_or_else(|_| "".to_string()),
            base_url,
        ),
    )]
    .into()
}

#[derive(Clone)]
struct BaseUrl(String);

pub async fn run_server(
    db: DB,
    addr: SocketAddr,
    base_url: &str,
    es: EmailSender,
    mut rx: tokio::sync::broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    let user_db = UserDB::new(db.clone());

    let auth_cache = Arc::new(users::AuthCache::new(db.clone()));
    let argon2 = Arc::new(Argon2::default());
    let email_sender = Arc::new(es);
    let basic_clients = Arc::new(build_oauth_clients(base_url));
    let slack_verifier = Arc::new(
        std::env::var("SLACK_SIGNING_SECRET")
            .ok()
            .map(|x| SlackVerifier::new(x).unwrap()),
    );

    let middleware_stack = ServiceBuilder::new()
        .layer(
            TraceLayer::new_for_http()
                .on_response(MyOnResponse {})
                .make_span_with(MyMakeSpan {})
                .on_request(()),
        )
        .layer(Extension(db.clone()))
        .layer(Extension(user_db))
        .layer(Extension(auth_cache.clone()))
        .layer(Extension(basic_clients))
        .layer(Extension(BaseUrl(base_url.to_string())))
        .layer(CookieManagerLayer::new());
    // build our application with a route
    let app = Router::new()
        .nest(
            "/api",
            Router::new()
                .nest(
                    "/w/:workspace_id",
                    Router::new()
                        .nest("/scripts", scripts::workspaced_service())
                        .nest("/jobs", jobs::workspaced_service())
                        .nest(
                            "/users",
                            users::workspaced_service()
                                .layer(Extension(argon2.clone()))
                                .layer(Extension(email_sender)),
                        )
                        .nest("/variables", variables::workspaced_service())
                        .nest("/oauth", oauth2::workspaced_service())
                        .nest("/resources", resources::workspaced_service())
                        .nest("/schedules", schedule::workspaced_service())
                        .nest("/groups", groups::workspaced_service())
                        .nest("/audit", audit::workspaced_service())
                        .nest("/acls", granular_acls::workspaced_service())
                        .nest("/workspaces", workspaces::workspaced_service())
                        .nest("/flows", flow::workspaced_service()),
                )
                .nest("/workspaces", workspaces::global_service())
                .nest(
                    "/users",
                    users::global_service().layer(Extension(argon2.clone())),
                )
                .nest("/workers", worker_ping::global_service())
                .nest("/scripts", scripts::global_service())
                .nest("/schedules", schedule::global_service())
                .route_layer(from_extractor::<users::Authed>())
                .route_layer(from_extractor::<users::Tokened>())
                .nest(
                    "/auth",
                    users::make_unauthed_service().layer(Extension(argon2)),
                )
                .nest(
                    "/oauth",
                    oauth2::global_service().layer(Extension(slack_verifier)),
                )
                .route("/version", get(git_v))
                .route("/openapi.yaml", get(openapi)),
        )
        .fallback(static_assets::static_handler.into_service())
        .layer(middleware_stack);

    let instance_name = rd_string(5);

    tracing::info!(addr = %addr.to_string(), instance = %instance_name, "server started listening");
    let server = axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .with_graceful_shutdown(async {
            rx.recv().await.ok();
            println!("Graceful shutdown of server");
        });

    tokio::spawn(async move { auth_cache.monitor().await });

    server.await?;
    Ok(())
}

pub fn monitor_db(db: &DB, timeout: i32, tx: tokio::sync::broadcast::Sender<()>) {
    let db1 = db.clone();
    let db2 = db.clone();

    let rx1 = tx.subscribe();
    let rx2 = tx.subscribe();

    tokio::spawn(async move { worker::restart_zombie_jobs_periodically(&db1, timeout, rx1).await });
    tokio::spawn(async move { users::delete_expired_items_perdiodically(&db2, rx2).await });
}

pub async fn run_workers(
    db: DB,
    addr: SocketAddr,
    timeout: i32,
    num_workers: i32,
    sleep_queue: u64,
    base_url: String,
    tx: tokio::sync::broadcast::Sender<()>,
) -> anyhow::Result<()> {
    let instance_name = rd_string(5);

    let mutex = Arc::new(Mutex::new(0));

    let sources: external_ip::Sources = external_ip::get_http_sources();
    let consensus = external_ip::ConsensusBuilder::new()
        .add_sources(sources)
        .build();

    let ip = consensus
        .get_consensus()
        .await
        .map(|x| x.to_string())
        .unwrap_or_else(|| "Unretrievable ip".to_string());

    let mut handles = Vec::new();
    for i in 1..(num_workers + 1) {
        let db1 = db.clone();
        let instance_name = instance_name.clone();
        let worker_name = format!("dt-worker-{}-{}", &instance_name, rd_string(5));
        let m1 = mutex.clone();
        let ip = ip.clone();
        let tx = tx.clone();
        let base_url = base_url.clone();
        handles.push(tokio::spawn(async move {
            tracing::info!(addr = %addr.to_string(), worker = %worker_name, "starting worker");
            worker::run_worker(
                &db1,
                timeout,
                &instance_name,
                worker_name,
                i as u64,
                num_workers as u64,
                m1,
                &ip,
                sleep_queue,
                &base_url,
                tx,
            )
            .await
        }));
    }
    futures::future::try_join_all(handles).await?;
    Ok(())
}

async fn git_v() -> &'static str {
    GIT_VERSION
}

async fn openapi() -> &'static str {
    include_str!("../openapi.yaml")
}

pub async fn shutdown_signal(tx: tokio::sync::broadcast::Sender<()>) -> anyhow::Result<()> {
    use std::io;
    use tokio::signal::unix::SignalKind;

    async fn terminate() -> io::Result<()> {
        tokio::signal::unix::signal(SignalKind::terminate())?
            .recv()
            .await;
        Ok(())
    }

    tokio::select! {
        _ = terminate() => {},
        _ = tokio::signal::ctrl_c() => {},
    }
    println!("signal received, starting graceful shutdown");
    let _ = tx.send(());
    Ok(())
}
