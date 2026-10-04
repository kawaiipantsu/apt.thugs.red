//! Private aggregate traffic statistics. Raw addresses and request headers never enter SQLite.
use crate::db::Database;
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use rusqlite::params;
use serde::Serialize;
use sha2::Sha256;
use std::net::IpAddr;
use zeroize::Zeroizing;

pub fn day(timestamp: i64) -> i64 {
    timestamp.div_euclid(86400)
}
pub fn today() -> i64 {
    day(Utc::now().timestamp())
}

pub struct ClientHasher(Zeroizing<Vec<u8>>);
impl ClientHasher {
    pub fn hash(&self, ip: IpAddr) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC key length");
        mac.update(b"xxc-aptd/analytics/client/v1\0");
        mac.update(ip.to_canonical().to_string().as_bytes());
        mac.finalize().into_bytes().into()
    }
}

pub struct Event {
    pub day: i64,
    pub kind: &'static str,
    pub client: Option<[u8; 32]>,
    pub asset: Option<String>,
    pub package: String,
    pub status: u16,
    pub bytes: u64,
    pub completed: bool,
}

#[derive(Default, Clone, Serialize)]
pub struct Counts {
    pub requests: u64,
    pub downloads: u64,
    pub ranges: u64,
    pub bytes: u64,
    pub errors: u64,
    pub not_modified: u64,
    pub interrupted: u64,
}
#[derive(Serialize)]
pub struct Daily {
    pub date: String,
    pub clients: u64,
    pub assets: u64,
    #[serde(flatten)]
    pub counts: Counts,
}
#[derive(Serialize)]
pub struct Ranked {
    pub name: String,
    pub requests: u64,
    pub downloads: u64,
    pub bytes: u64,
}
#[derive(Serialize)]
pub struct Breakdown {
    pub kind: String,
    pub requests: u64,
    pub bytes: u64,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub days: u32,
    pub collection_started: Option<i64>,
    pub clients: u64,
    pub assets: u64,
    pub totals: Counts,
    pub daily: Vec<Daily>,
    pub packages: Vec<Ranked>,
    pub popular_assets: Vec<Ranked>,
    pub breakdown: Vec<Breakdown>,
}
fn number(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = r.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn counts(r: &rusqlite::Row<'_>) -> rusqlite::Result<Counts> {
    Ok(Counts {
        requests: number(r, 0)?,
        downloads: number(r, 1)?,
        ranges: number(r, 2)?,
        bytes: number(r, 3)?,
        errors: number(r, 4)?,
        not_modified: number(r, 5)?,
        interrupted: number(r, 6)?,
    })
}
const SUMS: &str = "COALESCE(sum(requests),0),COALESCE(sum(downloads),0),COALESCE(sum(ranges),0),COALESCE(sum(bytes),0),COALESCE(sum(errors),0),COALESCE(sum(not_modified),0),COALESCE(sum(interrupted),0)";

impl Database {
    pub fn analytics_hasher(&self) -> Result<ClientHasher> {
        let conn = self.connect()?;
        let mut secret = Zeroizing::new(vec![0; 32]);
        getrandom::fill(&mut secret)
            .map_err(|_| anyhow::anyhow!("Cannot initialize analytics key"))?;
        conn.execute(
            "INSERT OR IGNORE INTO analytics_settings(id,secret,started) VALUES(1,?1,?2)",
            params![secret.as_slice(), Utc::now().timestamp()],
        )?;
        let saved: Vec<u8> = conn.query_row(
            "SELECT secret FROM analytics_settings WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(saved.len() == 32, "Invalid analytics key");
        Ok(ClientHasher(Zeroizing::new(saved)))
    }
    pub fn analytics_record(&self, events: &[Event]) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        for event in events {
            let download = event.kind == "packages" && event.status == 200 && event.completed;
            let bytes = event.bytes.min(i64::MAX as u64) as i64;
            tx.execute("INSERT INTO analytics_daily(day,kind,requests,downloads,ranges,bytes,errors,not_modified,interrupted) VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8) ON CONFLICT(day,kind) DO UPDATE SET requests=requests+1,downloads=downloads+excluded.downloads,ranges=ranges+excluded.ranges,bytes=bytes+excluded.bytes,errors=errors+excluded.errors,not_modified=not_modified+excluded.not_modified,interrupted=interrupted+excluded.interrupted",params![event.day,event.kind,download,event.status==206,bytes,event.status>=400,event.status==304,!event.completed])?;
            if let Some(client) = event.client {
                tx.execute(
                    "INSERT OR IGNORE INTO analytics_clients(day,client) VALUES(?1,?2)",
                    params![event.day, client.as_slice()],
                )?;
            }
            if let Some(asset) = &event.asset {
                ensure!(
                    asset.len() <= 2048 && asset.starts_with('/') && !asset.contains('?'),
                    "Invalid analytics asset"
                );
                tx.execute("INSERT INTO analytics_assets(day,path,kind,package,requests,downloads,bytes) VALUES(?1,?2,?3,?4,1,?5,?6) ON CONFLICT(day,path) DO UPDATE SET requests=requests+1,downloads=downloads+excluded.downloads,bytes=bytes+excluded.bytes",params![event.day,asset,event.kind,event.package,download,bytes])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn analytics_prune(&self, now_day: i64, retention_days: u32) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let cutoff = now_day - i64::from(retention_days) + 1;
        for table in ["analytics_daily", "analytics_clients", "analytics_assets"] {
            tx.execute(&format!("DELETE FROM {table} WHERE day<?1"), [cutoff])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn analytics_snapshot(&self, days: u32, now_day: i64) -> Result<Snapshot> {
        ensure!(
            matches!(days, 7 | 30 | 90),
            "Analytics days must be 7, 30 or 90"
        );
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let start = now_day - i64::from(days) + 1;
        let totals = tx.query_row(
            &format!("SELECT {SUMS} FROM analytics_daily WHERE day BETWEEN ?1 AND ?2"),
            params![start, now_day],
            counts,
        )?;
        let clients = tx.query_row(
            "SELECT count(DISTINCT client) FROM analytics_clients WHERE day BETWEEN ?1 AND ?2",
            params![start, now_day],
            |r| number(r, 0),
        )?;
        let assets = tx.query_row(
            "SELECT count(DISTINCT path) FROM analytics_assets WHERE day BETWEEN ?1 AND ?2",
            params![start, now_day],
            |r| number(r, 0),
        )?;
        let collection_started =
            tx.query_row("SELECT max(started) FROM analytics_settings", [], |r| {
                r.get(0)
            })?;
        let mut daily = Vec::with_capacity(days as usize);
        let mut daily_query =
            tx.prepare(&format!("SELECT {SUMS} FROM analytics_daily WHERE day=?1"))?;
        let mut clients_query =
            tx.prepare("SELECT count(*) FROM analytics_clients WHERE day=?1")?;
        let mut assets_query = tx.prepare("SELECT count(*) FROM analytics_assets WHERE day=?1")?;
        for d in start..=now_day {
            daily.push(Daily {
                date: DateTime::from_timestamp(d * 86400, 0)
                    .ok_or_else(|| anyhow::anyhow!("Invalid analytics date"))?
                    .format("%Y-%m-%d")
                    .to_string(),
                clients: clients_query.query_row([d], |r| number(r, 0))?,
                assets: assets_query.query_row([d], |r| number(r, 0))?,
                counts: daily_query.query_row([d], counts)?,
            });
        }
        let ranking = |column: &str, condition: &str, order: &str| -> Result<Vec<Ranked>> {
            let mut query=tx.prepare(&format!("SELECT {column},sum(requests),sum(downloads),sum(bytes) FROM analytics_assets WHERE day BETWEEN ?1 AND ?2 {condition} GROUP BY {column} ORDER BY {order} DESC,sum(requests) DESC,{column} LIMIT 8"))?;
            Ok(query
                .query_map(params![start, now_day], |r| {
                    Ok(Ranked {
                        name: r.get(0)?,
                        requests: number(r, 1)?,
                        downloads: number(r, 2)?,
                        bytes: number(r, 3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        };
        let packages = ranking("package", "AND package<>''", "sum(downloads)")?;
        let popular_assets = ranking("path", "", "sum(requests)")?;
        let mut query=tx.prepare("SELECT kind,sum(requests),sum(bytes) FROM analytics_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY kind ORDER BY sum(requests) DESC,kind")?;
        let breakdown = query
            .query_map(params![start, now_day], |r| {
                Ok(Breakdown {
                    kind: r.get(0)?,
                    requests: number(r, 1)?,
                    bytes: number(r, 2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Snapshot {
            days,
            collection_started,
            clients,
            assets,
            totals,
            daily,
            packages,
            popular_assets,
            breakdown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_deduplicate_retain_and_restart_without_raw_addresses() {
        let root = tempfile::tempdir().unwrap();
        let config =
            crate::config::Config::initialize(&root.path().join("config"), Some(root.path()))
                .unwrap();
        let db = Database::open(&config).unwrap();
        let hasher = db.analytics_hasher().unwrap();
        let ip: IpAddr = "192.0.2.33".parse().unwrap();
        let client = hasher.hash(ip);
        assert_eq!(client, db.analytics_hasher().unwrap().hash(ip));
        assert_eq!(client, hasher.hash("::ffff:192.0.2.33".parse().unwrap()));
        let now = today();
        let event = |d, status, completed| Event {
            day: d,
            kind: "packages",
            client: Some(client),
            asset: Some("/repo/pool/main/f/fixture/fixture_1_all.deb".into()),
            package: "fixture".into(),
            status,
            bytes: 100,
            completed,
        };
        db.analytics_record(&[
            event(now - 1, 200, true),
            event(now, 200, true),
            event(now, 206, true),
            event(now, 200, false),
        ])
        .unwrap();
        let s = db.analytics_snapshot(7, now).unwrap();
        assert_eq!(
            (
                s.totals.requests,
                s.totals.downloads,
                s.totals.ranges,
                s.totals.interrupted,
                s.totals.bytes
            ),
            (4, 2, 1, 1, 400)
        );
        assert_eq!((s.clients, s.assets), (1, 1));
        assert_eq!(s.daily.len(), 7);
        assert_eq!(s.daily[6].clients, 1);
        assert_eq!(s.packages[0].name, "fixture");
        db.analytics_prune(now, 1).unwrap();
        assert_eq!(db.analytics_snapshot(7, now).unwrap().totals.downloads, 1);
        assert_eq!(
            Database::open(&config)
                .unwrap()
                .analytics_snapshot(7, now)
                .unwrap()
                .clients,
            1
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("192.0.2.33") && !json.contains("secret"));
        assert!(db.analytics_snapshot(365, now).is_err());
    }
}
