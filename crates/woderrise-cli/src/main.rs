use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};
use woderrise_core::{BackupManager, Database, InstallationEngine, Manifest};

#[derive(Parser)]
#[command(name = "woderrise", about = "Safe declarative desktop package manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
    #[arg(long, default_value = "~/.local/share/woderrise")]
    data_dir: PathBuf,
}
#[derive(Subcommand)]
enum Command {
    Install {
        package: PathBuf,
    },
    Rollback {
        transaction: String,
        #[arg(long)]
        force: bool,
    },
    List,
    Lint {
        manifest: PathBuf,
    },
}
fn main() -> ExitCode {
    let cli = Cli::parse();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let data = if cli.data_dir.to_string_lossy().starts_with("~/") {
        home.join(cli.data_dir.to_string_lossy().trim_start_matches("~/"))
    } else {
        cli.data_dir
    };
    let result = (|| -> woderrise_core::Result<()> {
        match cli.command {
            Command::Lint { manifest } => {
                Manifest::from_file(manifest)?;
                println!("manifest is valid");
            }
            Command::List => {
                let db = Database::open(data.join("journal.db"))?;
                let mut s=db.conn.prepare("SELECT txn_id,package_id,version,status FROM transactions ORDER BY started_at DESC")?;
                for r in s.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })? {
                    let (id, p, v, status) = r?;
                    println!("{id}\t{p}\t{v}\t{status}");
                }
            }
            Command::Install { package } => {
                let db = Database::open(data.join("journal.db"))?;
                let engine = InstallationEngine {
                    db,
                    backups: BackupManager::new(data.join("backups"))?,
                    home,
                };
                println!("{}", engine.install(package, "user_cli")?);
            }
            Command::Rollback { transaction, force } => {
                let db = Database::open(data.join("journal.db"))?;
                let engine = InstallationEngine {
                    db,
                    backups: BackupManager::new(data.join("backups"))?,
                    home,
                };
                println!("{}", engine.rollback(&transaction, force)?);
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("woderrise: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
