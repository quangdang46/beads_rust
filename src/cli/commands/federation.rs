//! Federation / P2P Sync CLI commands.
//!
//! Manages federation peers for P2P synchronization.
//! Uses the canonical `federation_peers` table from schema.rs (v17+)
//! with parameterized queries (no SQL interpolation).

use clap::{Args, Subcommand};
use fsqlite::Connection;
use fsqlite_types::SqliteValue;

use crate::error::{BeadsError, Result};
use crate::output::OutputContext;
use crate::util::credentials::CredentialKey;
use chrono::Utc;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Federation subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum FederationCommand {
    /// Add a new federation peer.
    Add(FederationAddArgs),
    /// List all configured peers.
    List,
    /// Remove a peer by name or URL.
    Remove(FederationRemoveArgs),
    /// Sync with a peer (stub).
    Sync(FederationSyncArgs),
    /// Show peer details.
    Info(FederationInfoArgs),
}

/// Arguments for `federation add`.
#[derive(Args, Debug, Clone)]
pub struct FederationAddArgs {
    /// Peer name (used as primary key)
    pub name: String,
    /// Remote URL (e.g., https://beads.example.com)
    pub remote_url: String,
    /// Optional username for authentication
    #[arg(long)]
    pub username: Option<String>,
    /// Optional password for authentication
    #[arg(long)]
    pub password: Option<String>,
    /// Sovereignty tier: T1, T2, T3, or T4 (default: T3)
    #[arg(long, default_value = "T3")]
    pub sovereignty: String,
}

/// Arguments for `federation remove`.
#[derive(Args, Debug, Clone)]
pub struct FederationRemoveArgs {
    /// Name of the peer to remove
    pub name: String,
}

/// Arguments for `federation sync`.
#[derive(Args, Debug, Clone)]
pub struct FederationSyncArgs {
    /// Name of the peer to sync with
    pub name: String,
}

/// Arguments for `federation info`.
#[derive(Args, Debug, Clone)]
pub struct FederationInfoArgs {
    /// Name of the peer to inspect
    pub name: String,
}

/// A federation peer as stored in the database.
#[derive(Debug, Clone)]
struct FederationPeer {
    name: String,
    remote_url: String,
    username: String,
    sovereignty: String,
    last_sync: String,
    created_at: String,
    updated_at: String,
}

impl FederationPeer {
    fn from_row(row: &fsqlite::Row) -> Self {
        let get_text = |idx: usize| -> String {
            row.get(idx)
                .and_then(SqliteValue::as_text)
                .map(String::from)
                .unwrap_or_default()
        };
        Self {
            name: get_text(0),
            remote_url: get_text(1),
            username: get_text(2),
            sovereignty: get_text(4),
            last_sync: get_text(5),
            created_at: get_text(6),
            updated_at: get_text(7),
        }
    }
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

/// Path to the default beads database.
fn default_beads_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut dir = cwd;
    dir.push(".beads");
    dir
}

/// Open a connection to the beads database.
fn open_connection() -> Result<Connection> {
    let mut db_path = default_beads_dir();
    db_path.push("beads.db");
    let path_str = db_path.to_str().ok_or_else(|| BeadsError::Internal {
        message: "invalid beads directory path".to_string(),
    })?;
    Connection::open(path_str).map_err(BeadsError::Database)
}

/// Insert a new federation peer using parameterized queries.
fn federation_peers_insert(
    conn: &Connection,
    peer: &FederationPeer,
    encrypted_password: Option<Vec<u8>>,
) -> Result<()> {
    conn.execute_with_params(
        "INSERT OR REPLACE INTO federation_peers
         (name, remote_url, username, password_encrypted, sovereignty, last_sync, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        &[
            SqliteValue::from(peer.name.as_str()),
            SqliteValue::from(peer.remote_url.as_str()),
            SqliteValue::from(peer.username.as_str()),
            match &encrypted_password {
                Some(bytes) => SqliteValue::Blob(std::sync::Arc::from(bytes.as_slice())),
                None => SqliteValue::Null,
            },
            SqliteValue::from(peer.sovereignty.as_str()),
            SqliteValue::from(peer.last_sync.as_str()),
            SqliteValue::from(peer.created_at.as_str()),
            SqliteValue::from(peer.updated_at.as_str()),
        ],
    )
    .map_err(BeadsError::Database)?;
    Ok(())
}

/// List all federation peers.
fn federation_peers_list(conn: &Connection) -> Result<Vec<FederationPeer>> {
    let rows = conn
        .query(
            "SELECT name, remote_url, username, password_encrypted,
                    sovereignty, last_sync, created_at, updated_at
             FROM federation_peers
             ORDER BY name",
        )
        .map_err(BeadsError::Database)?;
    Ok(rows.iter().map(FederationPeer::from_row).collect())
}

/// Get a single peer by name.
fn federation_peers_get(conn: &Connection, name: &str) -> Result<Option<FederationPeer>> {
    let rows = conn
        .query_with_params(
            "SELECT name, remote_url, username, password_encrypted,
                    sovereignty, last_sync, created_at, updated_at
             FROM federation_peers
             WHERE name = ?1",
            &[SqliteValue::from(name)],
        )
        .map_err(BeadsError::Database)?;
    Ok(rows.first().map(|r| FederationPeer::from_row(r)))
}

/// Delete a peer by name.
fn federation_peers_delete(conn: &Connection, name: &str) -> Result<()> {
    conn.execute_with_params(
        "DELETE FROM federation_peers WHERE name = ?1",
        &[SqliteValue::from(name)],
    )
    .map_err(BeadsError::Database)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

/// Run the federation command.
pub fn run(command: &FederationCommand, ctx: &OutputContext) -> Result<()> {
    let conn = open_connection()?;

    match command {
        FederationCommand::Add(args) => cmd_add(&conn, args, ctx),
        FederationCommand::List => cmd_list(&conn, ctx),
        FederationCommand::Remove(args) => cmd_remove(&conn, args, ctx),
        FederationCommand::Sync(args) => cmd_sync(args, ctx),
        FederationCommand::Info(args) => cmd_info(&conn, args, ctx),
    }
}

/// Add a new peer.
fn cmd_add(
    conn: &Connection,
    args: &FederationAddArgs,
    ctx: &OutputContext,
) -> Result<()> {
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Encrypt password if provided
    let encrypted_password = if let Some(password) = &args.password {
        let beads_dir = default_beads_dir();
        let key = CredentialKey::load_or_create(&beads_dir).map_err(|e| BeadsError::Internal {
            message: format!("failed to load credential key: {e}"),
        })?;
        key.encrypt_password(password)
            .map_err(|e| BeadsError::Internal {
                message: format!("failed to encrypt password: {e}"),
            })?
    } else {
        None
    };

    let peer = FederationPeer {
        name: args.name.clone(),
        remote_url: args.remote_url.clone(),
        username: args.username.clone().unwrap_or_default(),
        sovereignty: args.sovereignty.clone(),
        last_sync: String::new(),
        created_at: now.clone(),
        updated_at: now,
    };

    federation_peers_insert(conn, &peer, encrypted_password)?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "name": peer.name,
                "remote_url": peer.remote_url,
                "sovereignty": peer.sovereignty,
            }))
            .map_err(|e| BeadsError::Internal {
                message: e.to_string()
            })?
        );
    } else {
        println!("Added peer: {} ({})", peer.name, peer.remote_url);
        println!("  Sovereignty: {}", peer.sovereignty);
    }

    Ok(())
}

/// List all peers.
fn cmd_list(conn: &Connection, ctx: &OutputContext) -> Result<()> {
    let peers = federation_peers_list(conn)?;

    if ctx.is_json() {
        let list: Vec<serde_json::Value> = peers
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "remote_url": p.remote_url,
                    "username": p.username,
                    "sovereignty": p.sovereignty,
                    "last_sync": p.last_sync,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "peers": list }))
                .map_err(|e| BeadsError::Internal {
                    message: e.to_string()
                })?
        );
    } else {
        if peers.is_empty() {
            println!("No federation peers configured.");
            return Ok(());
        }
        println!("Federation Peers ({}):", peers.len());
        for p in &peers {
            let last_sync = if p.last_sync.is_empty() { "never" } else { &p.last_sync };
            println!("  {} ({}) [{}] last sync: {}", p.name, p.remote_url, p.sovereignty, last_sync);
        }
    }

    Ok(())
}

/// Remove a peer.
fn cmd_remove(conn: &Connection, args: &FederationRemoveArgs, ctx: &OutputContext) -> Result<()> {
    let peer = federation_peers_get(conn, &args.name)?;
    let peer = match peer {
        Some(p) => p,
        None => {
            let msg = format!("peer not found: {}", args.name);
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "error": msg
                    }))
                    .map_err(|e| BeadsError::Internal {
                        message: e.to_string()
                    })?
                );
            } else {
                println!("{}", msg);
            }
            return Err(BeadsError::Internal { message: msg });
        }
    };

    federation_peers_delete(conn, &args.name)?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "removed": peer.name,
                "remote_url": peer.remote_url,
            }))
            .map_err(|e| BeadsError::Internal {
                message: e.to_string()
            })?
        );
    } else {
        println!("Removed peer: {} ({})", peer.name, peer.remote_url);
    }

    Ok(())
}

/// Sync with a peer (stub — P2P sync is planned for a future release).
fn cmd_sync(args: &FederationSyncArgs, ctx: &OutputContext) -> Result<()> {
    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": false,
                "error": "not yet implemented",
                "peer": args.name,
            }))
            .map_err(|e| BeadsError::Internal {
                message: e.to_string()
            })?
        );
    } else {
        println!("Sync with peer {}: not yet implemented", args.name);
        println!("P2P federation sync is planned for a future release.");
    }
    Ok(())
}

/// Show peer details.
fn cmd_info(conn: &Connection, args: &FederationInfoArgs, ctx: &OutputContext) -> Result<()> {
    let peer = federation_peers_get(conn, &args.name)?;

    match peer {
        Some(p) => {
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "name": p.name,
                        "remote_url": p.remote_url,
                        "username": p.username,
                        "sovereignty": p.sovereignty,
                        "last_sync": p.last_sync,
                        "created_at": p.created_at,
                        "updated_at": p.updated_at,
                    }))
                    .map_err(|e| BeadsError::Internal {
                        message: e.to_string()
                    })?
                );
            } else {
                println!("Peer Details:");
                println!("  Name: {}", p.name);
                println!("  URL: {}", p.remote_url);
                let username = if p.username.is_empty() {
                    "none".to_string()
                } else {
                    p.username.clone()
                };
                println!("  Username: {}", username);
                println!("  Sovereignty: {}", p.sovereignty);
                let last_sync = if p.last_sync.is_empty() {
                    "never".to_string()
                } else {
                    p.last_sync.clone()
                };
                println!("  Last Sync: {}", last_sync);
                println!("  Created: {}", p.created_at);
                println!("  Updated: {}", p.updated_at);
            }
            Ok(())
        }
        None => {
            let msg = format!("peer not found: {}", args.name);
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "error": msg
                    }))
                    .map_err(|e| BeadsError::Internal {
                        message: e.to_string()
                    })?
                );
            } else {
                println!("{}", msg);
            }
            Err(BeadsError::Internal { message: msg })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_federation_peer_from_row() {
        // Create a minimal in-memory DB and verify peer insertion/listing
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let db_path_str = db_path.to_str().unwrap();
        let conn = Connection::open(db_path_str).unwrap();

        // Create the federation_peers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS federation_peers (
                name TEXT PRIMARY KEY,
                remote_url TEXT NOT NULL,
                username TEXT,
                password_encrypted BLOB,
                sovereignty TEXT NOT NULL DEFAULT '',
                last_sync TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .unwrap();

        let peer = FederationPeer {
            name: "test-peer".to_string(),
            remote_url: "https://beads.example.com".to_string(),
            username: "admin".to_string(),
            sovereignty: "T2".to_string(),
            last_sync: String::new(),
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:00".to_string(),
        };

        federation_peers_insert(&conn, &peer, None).unwrap();
        let peers = federation_peers_list(&conn).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "test-peer");
        assert_eq!(peers[0].remote_url, "https://beads.example.com");
        assert_eq!(peers[0].sovereignty, "T2");

        // Test fetch by name
        let fetched = federation_peers_get(&conn, "test-peer").unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().remote_url, "https://beads.example.com");

        // Test non-existent
        let missing = federation_peers_get(&conn, "nonexistent").unwrap();
        assert!(missing.is_none());

        // Test delete
        federation_peers_delete(&conn, "test-peer").unwrap();
        let peers = federation_peers_list(&conn).unwrap();
        assert!(peers.is_empty());
    }
}
