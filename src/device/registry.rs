use super::ComputerId;
use anyhow::{ensure, Context, Result};
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{
    net::IpAddr,
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
/// Public device keys/IDs and short-lived registration quotas; no private keys or sessions.
pub struct Directory {
    state: Mutex<Connection>,
    maximum: usize,
}
struct Allocation {
    source: IpAddr,
    per_source: usize,
    global: usize,
    now: u64,
}
impl Directory {
    pub fn open(path: Option<&Path>, maximum: usize) -> Result<Self> {
        ensure!(maximum > 0, "Invalid device registry limit");
        let state = match path {
            Some(path) => Connection::open(path)?,
            None => Connection::open_in_memory()?,
        };
        state.busy_timeout(Duration::from_secs(2))?;
        state.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS devices (id TEXT PRIMARY KEY, key BLOB NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS registrations (occurred INTEGER NOT NULL, source TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS registrations_source ON registrations(source,occurred);")?;
        Ok(Self {
            state: Mutex::new(state),
            maximum,
        })
    }
    pub fn enroll(&self, public: &[u8; 32]) -> Result<ComputerId> {
        self.enroll_at(public, None)
    }
    pub fn enroll_limited(
        &self,
        public: &[u8; 32],
        source: IpAddr,
        per_source: usize,
        global: usize,
    ) -> Result<ComputerId> {
        self.enroll_at(
            public,
            Some(Allocation {
                source,
                per_source,
                global,
                now: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            }),
        )
    }
    pub fn prune_expired_allocations(&self) -> Result<()> {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .saturating_sub(3600);
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("Directory unavailable"))?
            .execute(
                "DELETE FROM registrations WHERE occurred<=?1",
                [i64::try_from(cutoff)?],
            )?;
        Ok(())
    }
    fn enroll_at(&self, public: &[u8; 32], allocation: Option<Allocation>) -> Result<ComputerId> {
        ensure!(public != &[0; 32], "Invalid device public key");
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Directory unavailable"))?;
        let transaction = state.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(id) = transaction
            .query_row(
                "SELECT id FROM devices WHERE key=?1",
                [public.as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return ComputerId::parse(&id);
        }
        let count: i64 =
            transaction.query_row("SELECT count(*) FROM devices", [], |row| row.get(0))?;
        ensure!(count < self.maximum as i64, "Device directory is full");
        if let Some(limit) = &allocation {
            ensure!(
                limit.per_source > 0 && limit.global > 0,
                "Invalid registration limits"
            );
            let cutoff = i64::try_from(limit.now.saturating_sub(3600))?;
            transaction.execute("DELETE FROM registrations WHERE occurred<=?1", [cutoff])?;
            let total: usize =
                transaction
                    .query_row("SELECT count(*) FROM registrations", [], |row| row.get(0))?;
            let source: usize = transaction.query_row(
                "SELECT count(*) FROM registrations WHERE source=?1",
                [limit.source.to_string()],
                |row| row.get(0),
            )?;
            ensure!(
                source < limit.per_source,
                "Too many new computers from this network. Try again later."
            );
            ensure!(
                total < limit.global,
                "New computer registration is busy. Try again later."
            );
        }
        for _ in 0..32 {
            let id = rand::thread_rng()
                .gen_range(100_000_000_000u64..1_000_000_000_000)
                .to_string();
            if transaction.execute(
                "INSERT OR IGNORE INTO devices (id,key) VALUES (?1,?2)",
                params![id, public.as_slice()],
            )? == 1
            {
                if let Some(limit) = &allocation {
                    transaction.execute(
                        "INSERT INTO registrations (occurred,source) VALUES (?1,?2)",
                        params![i64::try_from(limit.now)?, limit.source.to_string()],
                    )?;
                }
                transaction.commit()?;
                return ComputerId::parse(&id);
            }
        }
        anyhow::bail!("Could not allocate a computer ID")
    }
    pub fn lookup(&self, id: &ComputerId) -> Result<Option<[u8; 32]>> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Directory unavailable"))?;
        let key: Option<Vec<u8>> = state
            .query_row(
                "SELECT key FROM devices WHERE id=?1",
                [id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        key.map(|key| key.try_into().ok().context("Corrupt directory key"))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_limits_survive_restart_expire_and_never_charge_known_devices() {
        let path =
            std::env::temp_dir().join(format!("desklink-quota-{}.sqlite3", rand::random::<u64>()));
        let limits = |ip: &str, now| {
            Some(Allocation {
                source: ip.parse().unwrap(),
                per_source: 1,
                global: 2,
                now,
            })
        };
        let directory = Directory::open(Some(&path), 8).unwrap();
        let first = directory
            .enroll_at(&[1; 32], limits("192.0.2.1", 10_000))
            .unwrap();
        drop(directory);
        let directory = Directory::open(Some(&path), 8).unwrap();
        assert!(directory
            .enroll_at(&[2; 32], limits("192.0.2.1", 10_001))
            .is_err());
        assert_eq!(
            directory
                .enroll_at(&[1; 32], limits("192.0.2.1", 10_001))
                .unwrap(),
            first
        );
        directory
            .enroll_at(&[2; 32], limits("192.0.2.2", 10_001))
            .unwrap();
        assert!(directory
            .enroll_at(&[3; 32], limits("192.0.2.3", 10_002))
            .is_err());
        directory
            .enroll_at(&[3; 32], limits("192.0.2.1", 13_602))
            .unwrap();
        assert!(directory.lookup(&first).unwrap().is_some());
        drop(directory);
        std::fs::remove_file(path).unwrap();
    }
}
