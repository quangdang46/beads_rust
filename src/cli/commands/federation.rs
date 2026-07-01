//! Federation / P2P Sync CLI commands.
//!
//! Manages federation peers for P2P synchronization.

use crate::error::{BeadsError, Result};
use crate::output::OutputContext;
use chrono::Utc;
use clap::{Args, Subcommand};
use fsqlite::Connection;
use std::env;

// ---------------------------------------------------------------------------
// CLI arg types
// ---------------------------------------------------------------------------

/// Federation subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum FederationCommand {
    /// Add a new peer
    Add(FederationAddArgs),
    /// List all peers
    List,
    /// Remove a peer
    Remove(FederationRemoveArgs),
    /// Sync with a peer
    Sync(FederationSyncArgs),
    /// Show peer details
    Info(FederationInfoArgs),
}

/// Arguments for `federation add`.
#[derive(Args, Debug, Clone)]
pub struct FederationAddArgs {
    /// Unique peer name
    pub name: String,
    /// Peer URL (e.g., https://peer.example.com)
    pub url: String,
    /// Tier: T1 (local), T2 (team), T3 (org), T4 (public)
    #[arg(long)]
    pub tier: Option<String>,
    /// Authentication token (will be encrypted at rest)
    #[arg(long)]
    pub auth_token: Option<String>,
}

/// Arguments for `federation remove`.
#[derive(Args, Debug, Clone)]
pub struct FederationRemoveArgs {
    /// Peer ID to remove
    pub id: String,
}

/// Arguments for `federation sync`.
#[derive(Args, Debug, Clone)]
pub struct FederationSyncArgs {
    /// Peer ID to sync with
    pub id: String,
}

/// Arguments for `federation info`.
#[derive(Args, Debug, Clone)]
pub struct FederationInfoArgs {
    /// Peer ID or name to show
    pub id_or_name: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get the default database path.
fn default_db_path() -> String {
    env::var("BR_DATABASE_PATH")
        .or_else(|_| env::var("BEADS_DATABASE_PATH"))
        .unwrap_or_else(|_| ".beads/beads.db".to_string())
}

/// Get the current actor name.
fn get_actor() -> String {
    env::var("BR_ACTOR")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "federation".to_string())
}

/// Generate a unique ID for a peer.
fn generate_peer_id() -> String {
    use crate::util::id::generate_id;
    let now = Utc::now();
    generate_id("peer", Some("federation"), Some(&get_actor()), now)
}

/// Insert a new federation peer.
fn federation_peers_insert(conn: &Connection, peer: &FederationPeer) -> Result<()> {
    conn.execute(&format!(
        "INSERT INTO federation_peers (
            id, name, url, tier, auth_token_encrypted,
            last_sync_at, last_sync_hash, created_at,
            created_by, updated_at, enabled
        ) VALUES ('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', {})",
        escape_sql(&peer.id),
        escape_sql(&peer.name),
        escape_sql(&peer.url),
        escape_sql(&peer.tier),
        escape_sql(&peer.auth_token_encrypted),
        escape_sql(&peer.last_sync_at),
        escape_sql(&peer.last_sync_hash),
        escape_sql(&peer.created_at),
        escape_sql(&peer.created_by),
        escape_sql(&peer.updated_at),
        peer.enabled
    ))
    .map_err(BeadsError::Database)?;
    Ok(())
}

/// Escape single quotes for SQL.
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

/// List all enabled federation peers.
fn federation_peers_list(conn: &Connection) -> Result<Vec<FederationPeer>> {
    let rows = conn
        .query("SELECT * FROM federation_peers WHERE enabled=1")
        .map_err(BeadsError::Database)?;

    let mut peers = Vec::new();
    for row in rows {
        peers.push(FederationPeer::from_row(&row)?);
    }
    Ok(peers)
}

/// Get a peer by ID or name.
fn federation_peers_get(conn: &Connection, id_or_name: &str) -> Result<Option<FederationPeer>> {
    let id_escaped = id_or_name.replace('\'', "''");
    let sql = format!(
        "SELECT * FROM federation_peers WHERE id = '{}' OR (name = '{}' AND enabled = 1)",
        id_escaped, id_escaped
    );
    let rows = conn.query(&sql).map_err(BeadsError::Database)?;

    let mut iter = rows.into_iter();
    if let Some(row) = iter.next() {
        Ok(Some(FederationPeer::from_row(&row)?))
    } else {
        Ok(None)
    }
}

/// Delete a peer by ID.
fn federation_peers_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(&format!(
        "DELETE FROM federation_peers WHERE id = '{}'",
        escape_sql(id)
    ))
    .map_err(BeadsError::Database)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// Federation peer representation.
#[derive(Debug, Clone)]
struct FederationPeer {
    id: String,
    name: String,
    url: String,
    tier: String,
    auth_token_encrypted: String,
    last_sync_at: String,
    last_sync_hash: String,
    created_at: String,
    created_by: String,
    updated_at: String,
    enabled: i32,
}

impl FederationPeer {
    fn from_row(row: &fsqlite::Row) -> Result<Self> {
        let get_text = |idx| {
            row.get(idx)
                .and_then(fsqlite_types::SqliteValue::as_text)
                .map(String::from)
                .unwrap_or_default()
        };
        let get_int = |idx| -> i32 {
            row.get(idx)
                .and_then(fsqlite_types::SqliteValue::as_integer)
                .unwrap_or(1) as i32
        };
        Ok(Self {
            id: get_text(0),
            name: get_text(1),
            url: get_text(2),
            tier: get_text(3),
            auth_token_encrypted: get_text(4),
            last_sync_at: get_text(5),
            last_sync_hash: get_text(6),
            created_at: get_text(7),
            created_by: get_text(8),
            updated_at: get_text(9),
            enabled: get_int(10),
        })
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Run the federation command.
pub fn run(command: &FederationCommand, ctx: &OutputContext) -> Result<()> {
    let db_path = default_db_path();
    let conn = Connection::open(&db_path).map_err(BeadsError::Database)?;

    match command {
        FederationCommand::Add(args) => cmd_add(&conn, args, ctx),
        FederationCommand::List => cmd_list(&conn, ctx),
        FederationCommand::Remove(args) => cmd_remove(&conn, args, ctx),
        FederationCommand::Sync(args) => cmd_sync(args, ctx),
        FederationCommand::Info(args) => cmd_info(&conn, args, ctx),
    }
}

/// Add a new peer.
fn cmd_add(conn: &Connection, args: &FederationAddArgs, ctx: &OutputContext) -> Result<()> {
    let actor = get_actor();
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let tier = args.tier.as_deref().unwrap_or("T3");

    let peer = FederationPeer {
        id: generate_peer_id(),
        name: args.name.clone(),
        url: args.url.clone(),
        tier: tier.to_string(),
        auth_token_encrypted: args.auth_token.clone().unwrap_or_default(),
        last_sync_at: String::new(),
        last_sync_hash: String::new(),
        created_at: now.clone(),
        created_by: actor,
        updated_at: now,
        enabled: 1,
    };

    federation_peers_insert(conn, &peer)?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "peer_id": peer.id,
                "name": peer.name,
                "url": peer.url,
                "tier": peer.tier,
            }))
            .map_err(|e| BeadsError::Internal {
                message: e.to_string()
            })?
        );
    } else {
        println!("Added peer: {} ({})", peer.name, peer.id);
        println!("  URL: {}", peer.url);
        println!("  Tier: {}", peer.tier);
    }

    Ok(())
}

/// List all peers.
fn cmd_list(conn: &Connection, ctx: &OutputContext) -> Result<()> {
    let peers = federation_peers_list(conn)?;

    if ctx.is_json() {
        let peer_list: Vec<serde_json::Value> = peers
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "url": p.url,
                    "tier": p.tier,
                    "last_sync_at": p.last_sync_at,
                    "last_sync_hash": p.last_sync_hash,
                    "created_at": p.created_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "peers": peer_list,
                "count": peers.len(),
            }))
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
        println!(
            "{:<12} {:<20} {:<30} {:<6} {:<20}",
            "ID", "Name", "URL", "Tier", "Last Sync"
        );
        println!("{}", "-".repeat(90));
        for peer in &peers {
            let last_sync = if peer.last_sync_at.is_empty() {
                "never".to_string()
            } else {
                peer.last_sync_at.clone()
            };
            println!(
                "{:<12} {:<20} {:<30} {:<6} {:<20}",
                &peer.id[..12.min(peer.id.len())],
                &peer.name[..20.min(peer.name.len())],
                &peer.url[..30.min(peer.url.len())],
                peer.tier,
                &last_sync[..20.min(last_sync.len())]
            );
        }
    }

    Ok(())
}

/// Remove a peer.
fn cmd_remove(conn: &Connection, args: &FederationRemoveArgs, ctx: &OutputContext) -> Result<()> {
    let peer = federation_peers_get(conn, &args.id)?;
    let peer = match peer {
        Some(p) => p,
        None => {
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "error": "peer not found"
                    }))
                    .map_err(|e| BeadsError::Internal {
                        message: e.to_string()
                    })?
                );
            } else {
                println!("Peer not found: {}", args.id);
            }
            return Err(BeadsError::Internal {
                message: "peer not found".to_string(),
            });
        }
    };

    federation_peers_delete(conn, &args.id)?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "removed": peer.id,
                "name": peer.name,
            }))
            .map_err(|e| BeadsError::Internal {
                message: e.to_string()
            })?
        );
    } else {
        println!("Removed peer: {} ({})", peer.name, peer.id);
    }

    Ok(())
}

/// Sync with a peer.
fn cmd_sync(args: &FederationSyncArgs, ctx: &OutputContext) -> Result<()> {
    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "peer_id": args.id,
                "status": "not_implemented",
                "message": "P2P sync not yet implemented"
            }))
            .map_err(|e| BeadsError::Internal {
                message: e.to_string()
            })?
        );
    } else {
        println!("Sync with peer {}: not yet implemented", args.id);
        println!("P2P federation sync is planned for a future release.");
    }
    Ok(())
}

/// Show peer details.
fn cmd_info(conn: &Connection, args: &FederationInfoArgs, ctx: &OutputContext) -> Result<()> {
    let peer = federation_peers_get(conn, &args.id_or_name)?;

    match peer {
        Some(p) => {
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "url": p.url,
                        "tier": p.tier,
                        "auth_configured": !p.auth_token_encrypted.is_empty(),
                        "last_sync_at": p.last_sync_at,
                        "last_sync_hash": p.last_sync_hash,
                        "created_at": p.created_at,
                        "created_by": p.created_by,
                        "updated_at": p.updated_at,
                        "enabled": p.enabled == 1,
                    }))
                    .map_err(|e| BeadsError::Internal {
                        message: e.to_string()
                    })?
                );
            } else {
                println!("Peer Details:");
                println!("  ID: {}", p.id);
                println!("  Name: {}", p.name);
                println!("  URL: {}", p.url);
                println!("  Tier: {}", p.tier);
                let auth_status = if p.auth_token_encrypted.is_empty() {
                    "not configured"
                } else {
                    "[configured]"
                };
                println!("  Auth Token: {}", auth_status);
                let last_sync = if p.last_sync_at.is_empty() {
                    "never"
                } else {
                    &p.last_sync_at
                };
                println!("  Last Sync: {}", last_sync);
                let sync_hash = if p.last_sync_hash.is_empty() {
                    "none"
                } else {
                    &p.last_sync_hash
                };
                println!("  Last Sync Hash: {}", sync_hash);
                println!("  Created: {} by {}", p.created_at, p.created_by);
                println!("  Updated: {}", p.updated_at);
                let enabled = if p.enabled == 1 { "yes" } else { "no" };
                println!("  Enabled: {}", enabled);
            }
            Ok(())
        }
        None => {
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "error": "peer not found"
                    }))
                    .map_err(|e| BeadsError::Internal {
                        message: e.to_string()
                    })?
                );
            } else {
                println!("Peer not found: {}", args.id_or_name);
            }
            Err(BeadsError::Internal {
                message: "peer not found".to_string(),
            })
        }
    }
}
