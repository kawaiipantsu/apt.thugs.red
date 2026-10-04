use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version = xxc_aptd_core::BUILD_VERSION, about = "XXC-APTD local repository administration")]
struct Arguments {
    #[arg(long, global = true, default_value = "/run/xxc-aptd/admin.sock")]
    socket: PathBuf,
    /// Select a configured suite for package, upload and publication operations.
    #[arg(long, global = true)]
    suite: Option<String>,
    #[arg(long, global = true, default_value = "/etc/xxc/aptd.conf")]
    config: PathBuf,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    quiet: bool,
    #[arg(long)]
    about: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Show service version, current generation and package count.
    Status,
    /// Check database and initialized directories.
    Health,
    /// Query published and staged package records.
    Package {
        #[command(subcommand)]
        command: PackageCommands,
    },
    /// Stream Debian packages into quarantine and explicitly stage them.
    Upload {
        #[command(subcommand)]
        command: UploadCommands,
    },
    /// Publish, verify, reindex or restore signed generations.
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
    },
    /// Manage local HTTP accounts (socket authorization required).
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
    /// Read XXC Trust connection and X.509 inventory (administrator only).
    Trust {
        #[command(subcommand)]
        command: TrustCommands,
    },
    /// Manage OpenPGP public keys held by XXC Trust; no private exports.
    Key {
        #[command(subcommand)]
        command: KeyCommands,
    },
    /// Inspect background job results.
    Jobs {
        #[command(subcommand)]
        command: JobCommands,
    },
    /// Read recent administrative audit events.
    Audit {
        #[command(subcommand)]
        command: AuditCommands,
    },
    /// Read effective configuration or validate a local file.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}
#[derive(Subcommand)]
enum PackageCommands {
    List,
    Search { query: String },
    Show { id: String },
    Import { file: PathBuf },
}
#[derive(Subcommand)]
enum UploadCommands {
    Add { file: PathBuf },
    List,
    Inspect { id: String },
    Stage { id: String },
}
#[derive(Subcommand)]
enum RepoCommands {
    Status,
    Verify,
    Reindex,
    /// Display the publication diff and its review token.
    Diff,
    /// Publish the exact selection approved by repo diff.
    Publish {
        #[arg(long)]
        review_token: String,
    },
    Generations,
    Rollback {
        generation: String,
    },
}
#[derive(Subcommand)]
enum UserCommands {
    List,
    /// Create an account; passwords are prompted without echo.
    Add {
        username: String,
        #[arg(long, value_enum, default_value = "viewer")]
        role: xxc_aptd_core::auth::Role,
        #[arg(long)]
        password_stdin: bool,
    },
    /// Reset a password and revoke sessions.
    Passwd {
        id: String,
        #[arg(long)]
        password_stdin: bool,
    },
    /// Set a role and revoke sessions.
    Role {
        id: String,
        #[arg(value_enum)]
        role: xxc_aptd_core::auth::Role,
    },
    Disable {
        id: String,
    },
    Enable {
        id: String,
    },
    Delete {
        id: String,
    },
}
#[derive(Subcommand)]
enum TrustCommands {
    Status,
    Authorities,
    Templates,
    Certificates {
        #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=1_000_000))]
        page: u32,
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long, default_value="", value_parser=["", "active", "expiring", "expired", "revoked"])]
        status: String,
    },
}
#[derive(Subcommand)]
enum KeyCommands {
    List {
        #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u32).range(1..=1_000_000))]
        page: u32,
        #[arg(long, default_value = "")]
        query: String,
    },
    Show {
        id: String,
    },
    /// Generate remotely; this public identity will appear in distributed keys.
    Generate {
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: String,
        #[arg(long,default_value="Ed25519",value_parser=["Ed25519","RSA-3072","RSA-4096"])]
        algorithm: String,
        #[arg(long,default_value_t=365,value_parser=clap::value_parser!(u32).range(1..=3650))]
        days: u32,
    },
    /// Write public key bytes to a new file; never overwrites an existing file.
    Export {
        id: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        armor: bool,
    },
    /// Check public key material against its fingerprint (and pin when active).
    Verify {
        id: String,
    },
}
#[derive(Subcommand)]
enum JobCommands {
    List,
    Show { id: String },
}
#[derive(Subcommand)]
enum AuditCommands {
    Tail,
}
#[derive(Subcommand)]
enum ConfigCommands {
    Show,
    Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
    let a = Arguments::parse();
    if a.about {
        println!("{}", xxc_aptd_core::ABOUT);
        return Ok(());
    }
    let command = a.command.context("Select a command; see --help")?;
    if matches!(
        command,
        Commands::Config {
            command: ConfigCommands::Validate
        }
    ) {
        xxc_aptd_core::config::Config::load(&a.config)?;
        if !a.quiet {
            println!("{}", json!({"valid":true}));
        }
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .unix_socket(a.socket.as_path())
        .timeout(std::time::Duration::from_secs(960))
        .build()?;
    let base = "http://localhost/api/v1";
    let mut upload = None;
    let mut body = None;
    let mut query = None;
    let mut trust_query = None;
    let mut key_query = None;
    let mut key_export = None;
    let path = match command {
        Commands::Status
        | Commands::Repo {
            command: RepoCommands::Status,
        } => "status".into(),
        Commands::Health => "health".into(),
        Commands::Key { command } => match command {
            KeyCommands::List { page, query } => {
                let q = xxc_aptd_core::trust::openpgp::KeyQuery { page, q: query };
                q.validate()?;
                key_query = Some(q);
                "keys".into()
            }
            KeyCommands::Show { id } => format!("keys/{}", segment(&id)?),
            KeyCommands::Verify { id } => format!("keys/{}/verify", segment(&id)?),
            KeyCommands::Generate {
                name,
                email,
                algorithm,
                days,
            } => {
                let input = xxc_aptd_core::trust::openpgp::GenerateKey {
                    name,
                    email,
                    algorithm,
                    days,
                };
                input.validate()?;
                body = Some(json!(input));
                "keys".into()
            }
            KeyCommands::Export { id, output, armor } => {
                key_export = Some(output);
                format!(
                    "keys/{}/public?format={}",
                    segment(&id)?,
                    if armor { "armor" } else { "binary" }
                )
            }
        },
        Commands::Trust { command } => match command {
            TrustCommands::Status => "trust/status".into(),
            TrustCommands::Authorities => "trust/authorities".into(),
            TrustCommands::Templates => "trust/templates".into(),
            TrustCommands::Certificates {
                page,
                query,
                status,
            } => {
                let q = xxc_aptd_core::trust::CertificateQuery {
                    page,
                    q: query,
                    status,
                };
                q.validate()?;
                trust_query = Some(q);
                "trust/certificates".into()
            }
        },
        Commands::Package {
            command: PackageCommands::List,
        }
        | Commands::Upload {
            command: UploadCommands::List,
        } => "packages".into(),
        Commands::Package {
            command: PackageCommands::Search { query: q },
        } => {
            query = Some(q);
            "packages".into()
        }
        Commands::Package {
            command: PackageCommands::Show { id },
        }
        | Commands::Upload {
            command: UploadCommands::Inspect { id },
        } => format!("packages/{}", segment(&id)?),
        Commands::Package {
            command: PackageCommands::Import { file },
        }
        | Commands::Upload {
            command: UploadCommands::Add { file },
        } => {
            upload = Some(file);
            "uploads".into()
        }
        Commands::Upload {
            command: UploadCommands::Stage { id },
        } => {
            body = Some(json!({}));
            format!("uploads/{}/stage", segment(&id)?)
        }
        Commands::Repo {
            command: RepoCommands::Publish { review_token },
        } => {
            body = Some(json!({"review_token":review_token}));
            "repository/publish".into()
        }
        Commands::Repo {
            command: RepoCommands::Verify,
        } => {
            body = Some(json!({}));
            "repository/verify".into()
        }
        Commands::Repo {
            command: RepoCommands::Reindex,
        } => {
            body = Some(json!({}));
            "repository/reindex".into()
        }
        Commands::Repo {
            command: RepoCommands::Rollback { generation },
        } => {
            body = Some(json!({"generation":generation}));
            "repository/rollback".into()
        }
        Commands::Repo {
            command: RepoCommands::Generations,
        } => "repository/generations".into(),
        Commands::Repo {
            command: RepoCommands::Diff,
        } => "repository/diff".into(),
        Commands::User {
            command: UserCommands::List,
        } => "users".into(),
        Commands::User {
            command:
                UserCommands::Add {
                    username,
                    role,
                    password_stdin,
                },
        } => {
            let password = read_password(password_stdin)?;
            body = Some(json!({"username":username,"role":role,"password":password.as_str()}));
            "users".into()
        }
        Commands::User {
            command: UserCommands::Passwd { id, password_stdin },
        } => {
            let password = read_password(password_stdin)?;
            body = Some(json!({"action":"password","password":password.as_str()}));
            format!("users/{}", segment(&id)?)
        }
        Commands::User {
            command: UserCommands::Role { id, role },
        } => {
            body = Some(json!({"action":"role","role":role}));
            format!("users/{}", segment(&id)?)
        }
        Commands::User { command } => {
            let (id, action) = match command {
                UserCommands::Enable { id } => (id, "enable"),
                UserCommands::Disable { id } => (id, "disable"),
                UserCommands::Delete { id } => (id, "delete"),
                _ => unreachable!(),
            };
            body = Some(json!({"action":action}));
            format!("users/{}", segment(&id)?)
        }
        Commands::Jobs {
            command: JobCommands::List,
        } => "jobs".into(),
        Commands::Jobs {
            command: JobCommands::Show { id },
        } => format!("jobs/{}", segment(&id)?),
        Commands::Audit {
            command: AuditCommands::Tail,
        } => "audit".into(),
        Commands::Config {
            command: ConfigCommands::Show,
        } => "config".into(),
        Commands::Config {
            command: ConfigCommands::Validate,
        } => unreachable!(),
    };
    let url = format!("{base}/{path}");
    let request = if let Some(file) = upload {
        let file = tokio::fs::File::open(file).await?;
        let length = file.metadata().await?.len();
        client
            .post(url)
            .header("content-type", "application/vnd.debian.binary-package")
            .header("content-length", length)
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(file),
            ))
    } else if let Some(mut body) = body {
        let request = client.post(url).json(&body);
        if let Some(Value::String(password)) = body.get_mut("password") {
            zeroize::Zeroize::zeroize(password);
        }
        request
    } else {
        let mut r = client.get(url);
        if let Some(q) = key_query {
            r = r.query(&q);
        }
        if let Some(q) = trust_query {
            r = r.query(&q);
        }
        if let Some(q) = query {
            r = r.query(&[("q", q)]);
        }
        r
    };
    let request = if let Some(suite) = a.suite {
        request.query(&[("suite", suite)])
    } else {
        request
    };
    let response = request
        .send()
        .await
        .context("Cannot reach daemon; check --socket, group membership and service status")?;
    let status = response.status();
    if status.is_success()
        && let Some(path) = key_export
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let bytes = response.bytes().await?;
        anyhow::ensure!(
            bytes.len() <= 2 * 1024 * 1024,
            "Public key response too large"
        );
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        if !a.quiet {
            println!("{}", json!({"written":true,"bytes":bytes.len()}));
        }
        return Ok(());
    }
    let value: Value = response
        .json()
        .await
        .context("Daemon returned invalid JSON")?;
    if !status.is_success() {
        bail!("API request failed ({status}): {value}");
    }
    if !a.quiet {
        if a.json {
            println!("{value}");
        } else {
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }
    Ok(())
}
fn segment(s: &str) -> Result<&str> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        bail!("Invalid identifier");
    }
    Ok(s)
}

fn read_password(stdin: bool) -> Result<zeroize::Zeroizing<String>> {
    let value = if stdin {
        use std::io::Read;
        let mut value = zeroize::Zeroizing::new(String::new());
        std::io::stdin().take(1027).read_to_string(&mut value)?;
        if value.ends_with('\n') {
            value.pop();
            if value.ends_with('\r') {
                value.pop();
            }
        }
        value
    } else {
        let first = zeroize::Zeroizing::new(rpassword::prompt_password("New password: ")?);
        let second = zeroize::Zeroizing::new(rpassword::prompt_password("Confirm password: ")?);
        anyhow::ensure!(*first == *second, "Passwords did not match");
        first
    };
    anyhow::ensure!(
        (12..=1024).contains(&value.len()) && !value.contains('\0'),
        "Password must contain 12..1024 bytes without NUL"
    );
    Ok(value)
}
