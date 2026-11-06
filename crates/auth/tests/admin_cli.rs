//! End-to-end tests of the `aps-admin` binary against a scratch database.

use std::process::Command;

use aps_auth::{authenticate_api_key, ApiKey};
use aps_store::{CallerTable, SqliteStore};

fn admin(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_aps-admin"))
        .args(args)
        .output()
        .expect("run aps-admin");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn create_list_rotate_revoke_and_verify() {
    let dir = std::env::temp_dir().join(format!("aps-admin-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("ledger.sqlite");
    let db_s = db.to_str().unwrap();

    let (code, token, _) = admin(&["create-caller", "--db", db_s, "--caller", "agent-1"]);
    assert_eq!(code, 0);
    let token = token.trim().to_string();
    let key = ApiKey::parse(&token).expect("stdout is exactly the token");
    {
        let store = SqliteStore::open(&db).unwrap();
        assert_eq!(authenticate_api_key(&store, &token).unwrap(), "agent-1");
        let listed = store.list_callers().unwrap();
        assert_eq!(listed[0].key_sha256, key.digest());
    }

    let (code, listing, _) = admin(&["list-callers", "--db", db_s]);
    assert_eq!(code, 0);
    assert!(listing.starts_with("agent-1\tactive\t"));
    assert!(
        !listing.contains(&token[4..]),
        "the key never appears in a listing"
    );

    let (code, rotated, _) = admin(&["rotate-caller", "--db", db_s, "--caller", "agent-1"]);
    assert_eq!(code, 0);
    {
        let store = SqliteStore::open(&db).unwrap();
        assert!(authenticate_api_key(&store, &token).is_err());
        assert!(authenticate_api_key(&store, rotated.trim()).is_ok());
    }

    assert_eq!(
        admin(&["revoke-caller", "--db", db_s, "--caller", "agent-1"]).0,
        0
    );
    assert_eq!(
        admin(&["revoke-caller", "--db", db_s, "--caller", "agent-1"]).0,
        1
    );
    assert_eq!(
        admin(&["create-caller", "--db", db_s, "--caller", "agent-1"]).0,
        1
    );

    let (code, report, _) = admin(&["verify-journal", "--db", db_s]);
    assert_eq!(code, 0);
    assert!(report.contains("events: 0"));
    assert!(report.contains("conservation: ok"));

    let backup = dir.join("backup.sqlite");
    assert_eq!(
        admin(&["backup", "--db", db_s, "--out", backup.to_str().unwrap()]).0,
        0
    );
    assert!(backup.exists());

    assert_eq!(admin(&[]).0, 2);
    assert_eq!(admin(&["list-callers"]).0, 2);
    assert_eq!(admin(&["bogus", "--db", db_s]).0, 1);
    let _ = std::fs::remove_dir_all(&dir);
}
