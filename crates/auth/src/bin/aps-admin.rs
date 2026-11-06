//! `aps-admin`: operator CLI for callers and the journal.
//!
//! ```text
//! aps-admin create-caller  --db PATH --caller ID
//! aps-admin rotate-caller  --db PATH --caller ID
//! aps-admin revoke-caller  --db PATH --caller ID
//! aps-admin list-callers   --db PATH
//! aps-admin verify-journal --db PATH
//! aps-admin backup         --db PATH --out PATH
//! ```
//!
//! `create-caller` and `rotate-caller` print the new API key on stdout
//! exactly once; only its SHA-256 is stored. Nothing else ever prints key
//! material. Exit codes: 0 success, 1 failure, 2 usage error.

use std::process::ExitCode;

use aps_store::{to_hex, CallerTable, OpenOptions, PersistentLedger, SqliteStore};

const USAGE: &str = "usage: aps-admin <create-caller|rotate-caller|revoke-caller|list-callers|verify-journal|backup> --db PATH [--caller ID] [--out PATH]";

struct Args {
    command: String,
    db: String,
    caller: Option<String>,
    out: Option<String>,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut iter = raw.iter();
    let command = iter.next().ok_or("missing command")?.clone();
    let (mut db, mut caller, mut out) = (None, None, None);
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?
            .clone();
        match flag.as_str() {
            "--db" => db = Some(value),
            "--caller" => caller = Some(value),
            "--out" => out = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(Args {
        command,
        db: db.ok_or("--db is required")?,
        caller,
        out,
    })
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn need_caller(args: &Args) -> Result<&str, String> {
    args.caller
        .as_deref()
        .ok_or_else(|| "--caller is required".to_string())
}

fn run(args: &Args) -> Result<(), String> {
    let mut store = SqliteStore::open(&args.db).map_err(|e| e.to_string())?;
    match args.command.as_str() {
        "create-caller" => {
            let caller = need_caller(args)?;
            let key = aps_auth::create_caller(&mut store, caller, unix_now())
                .map_err(|e| e.to_string())?;
            eprintln!("created caller {caller}; the key below is shown once and only its SHA-256 is stored");
            println!("{}", key.token());
        }
        "rotate-caller" => {
            let caller = need_caller(args)?;
            let key = aps_auth::rotate_caller(&mut store, caller).map_err(|e| e.to_string())?;
            eprintln!("rotated caller {caller}; the old key no longer works");
            println!("{}", key.token());
        }
        "revoke-caller" => {
            let caller = need_caller(args)?;
            if !store
                .revoke_caller(caller, unix_now())
                .map_err(|e| e.to_string())?
            {
                return Err(format!("caller {caller} is unknown or already revoked"));
            }
            println!("revoked {caller}");
        }
        "list-callers" => {
            for record in store.list_callers().map_err(|e| e.to_string())? {
                let status = match record.revoked_at {
                    None => "active".to_string(),
                    Some(at) => format!("revoked@{at}"),
                };
                // An 8-hex digest prefix identifies a key without exposing it.
                let prefix = &to_hex(&record.key_sha256)[..8];
                println!(
                    "{}\t{}\tcreated@{}\tdigest:{}",
                    record.caller_id, status, record.created_at, prefix
                );
            }
        }
        "verify-journal" => {
            let ledger =
                PersistentLedger::open(store, OpenOptions::default()).map_err(|e| e.to_string())?;
            let report = ledger.open_report();
            let projection = ledger.projection().map_err(|e| e.to_string())?;
            println!("events: {}", report.events_replayed);
            println!("head_seq: {}", report.head.seq);
            println!("head_hash: {}", report.head.hash_hex());
            match report.snapshot_seq {
                Some(seq) => println!("snapshot_seq: {seq}"),
                None => println!("snapshot_seq: none"),
            }
            println!("accounts: {}", projection.accounts.len());
            println!("jobs: {}", projection.jobs.len());
            println!(
                "conservation: {}",
                if ledger.is_conserved() {
                    "ok"
                } else {
                    "VIOLATED"
                }
            );
        }
        "backup" => {
            let out = args.out.as_deref().ok_or("--out is required")?;
            store.backup_to(out).map_err(|e| e.to_string())?;
            println!("backup written");
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("aps-admin: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("aps-admin: {message}");
            ExitCode::from(1)
        }
    }
}
