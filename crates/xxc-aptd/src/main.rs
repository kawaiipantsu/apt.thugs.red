mod logging;
mod system;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use fs2::FileExt;
use std::{
    fs,
    os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
use tokio::{
    net::{TcpListener, UnixListener},
    signal::unix::{SignalKind, signal},
};
use xxc_aptd_core::{config::Config, db::Database, repository};

#[derive(Parser)]
#[command(version = xxc_aptd_core::BUILD_VERSION, about = "XXC-APTD — signed Debian repository service")]
struct Arguments {
    #[arg(long, global = true, default_value = "/etc/xxc/aptd.conf")]
    config: PathBuf,
    #[arg(long, help = "Print project authorship and purpose")]
    about: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Initialize missing configuration and directories without replacing keys.
    Init {
        #[arg(long)]
        system: bool,
        #[arg(long, conflicts_with = "system")]
        root: Option<PathBuf>,
    },
    /// Validate configuration (unknown fields fail).
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Start public HTTP, authenticated administrative HTTP and the local socket.
    Serve {
        #[arg(long, help = "Permit root only for disposable development tests")]
        allow_root: bool,
    },
}
#[derive(Subcommand)]
enum ConfigCommand {
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Arguments::parse();
    if args.about {
        println!("{}", xxc_aptd_core::ABOUT);
        return Ok(());
    }
    match args
        .command
        .unwrap_or(Commands::Serve { allow_root: false })
    {
        Commands::Init {
            system: system_mode,
            root,
        } => {
            if system_mode {
                system::accounts()?;
            }
            let c = Config::initialize(&args.config, root.as_deref())?;
            if system_mode {
                system::ownership(&c, &args.config)?;
            }
            println!("Initialization complete; existing configuration and keys preserved.");
        }
        Commands::Config {
            command: ConfigCommand::Check,
        } => {
            Config::load(&args.config)?;
            println!("Configuration is valid.");
        }
        Commands::Serve { allow_root } => {
            // SAFETY: geteuid has no preconditions.
            ensure!(
                unsafe { libc::geteuid() } != 0 || allow_root,
                "Run as xxc-aptd; use --allow-root only in disposable tests"
            );
            serve(Config::load(&args.config)?).await?;
        }
    }
    Ok(())
}
async fn serve(c: Config) -> Result<()> {
    c.check_directories()?;
    logging::initialize(&c)?;
    if c.admin.enabled && !c.server.admin_listen.ip().is_loopback() {
        tracing::warn!(
            "Administrative HTTP is bound beyond loopback with explicit opt-in; authentication remains required; use a trusted test network or an upstream TLS proxy"
        );
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(c.paths.repository.join(".daemon.lock"))?;
    lock.try_lock_exclusive()
        .context("Another daemon owns this repository")?;
    let db = Database::open(&c)?;
    if let Some(m) = repository::current(&c)? {
        repository::verify(&c, &m)
            .context("Active repository is inconsistent; inspect before starting")?;
    }
    repository::reconcile(&c, &db)?;
    if c.signing.fingerprint.is_empty() {
        tracing::warn!("signing.fingerprint is empty; publication disabled until configured");
    }
    let public = TcpListener::bind(c.server.public_listen)
        .await
        .context("Cannot bind public HTTP listener")?;
    let admin = TcpListener::bind(c.server.admin_listen)
        .await
        .context("Cannot bind admin HTTP listener")?;
    let socket = c.server.admin_socket.clone();
    if let Ok(meta) = fs::symlink_metadata(&socket) {
        ensure!(
            meta.file_type().is_socket(),
            "admin_socket exists and is not a Unix socket"
        );
        ensure!(
            tokio::net::UnixStream::connect(&socket).await.is_err(),
            "admin_socket is already serving"
        );
        fs::remove_file(&socket)?;
    }
    let unix = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o660))?;
    let state = xxc_aptd_web::State::new(c, db)?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let shutdown = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.changed().await;
    };
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(
        axum::serve(public, xxc_aptd_web::public::router(state.clone()))
            .with_graceful_shutdown(shutdown(shutdown_rx.clone()))
            .into_future(),
    );
    tasks.spawn(
        axum::serve(
            admin,
            xxc_aptd_web::admin::router(state.clone())
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown(shutdown_rx.clone()))
        .into_future(),
    );
    tasks.spawn(
        axum::serve(unix, xxc_aptd_web::api::router(state.clone()))
            .with_graceful_shutdown(shutdown(shutdown_rx))
            .into_future(),
    );
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    tracing::info!(
        version = xxc_aptd_core::VERSION,
        "public HTTP and local management ready"
    );
    let mut failed = false;
    tokio::select! {
        _=term.recv()=>{},_=interrupt.recv()=>{},
        result=tasks.join_next()=>{tracing::error!(?result,"HTTP listener stopped unexpectedly");failed=true;}
    }
    tracing::info!("draining HTTP requests and publication job");
    let _ = shutdown_tx.send(true);
    while let Some(result) = tasks.join_next().await {
        result??;
    }
    let _permit = state.publisher.acquire().await?;
    fs::remove_file(socket)?;
    drop(lock);
    ensure!(!failed, "A listener failed");
    Ok(())
}
