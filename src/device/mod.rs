//! Persistent device identity and public, relay-scoped computer addresses.
mod ownership;
pub mod registry;
use anyhow::{ensure, Context, Result};
pub use ownership::{answer_challenge, make_challenge, verify_answer};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ComputerId(String);
impl ComputerId {
    pub fn parse(input: &str) -> Result<Self> {
        ensure!(input.len() <= 32, "Enter a 12-digit computer ID");
        let digits: String = input.chars().filter(|c| *c != ' ' && *c != '-').collect();
        ensure!(
            digits.len() == 12
                && digits.bytes().all(|c| c.is_ascii_digit())
                && !digits.starts_with('0'),
            "Enter a 12-digit computer ID"
        );
        Ok(Self(digits))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ComputerId {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}
impl From<ComputerId> for String {
    fn from(id: ComputerId) -> Self {
        id.0
    }
}
impl fmt::Display for ComputerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            &self.0[0..3],
            &self.0[3..6],
            &self.0[6..9],
            &self.0[9..12]
        )
    }
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Identity {
    private: [u8; 32],
    #[zeroize(skip)]
    public: [u8; 32],
    #[zeroize(skip)]
    root: PathBuf,
}
impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("private", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
impl Identity {
    pub fn default_root() -> Result<PathBuf> {
        Ok(
            directories::ProjectDirs::from("dev", "DeskLink", "DeskLink")
                .context("No user data directory")?
                .data_local_dir()
                .to_owned(),
        )
    }
    pub fn load_or_create(root: &Path) -> Result<Self> {
        fs::create_dir_all(root).context("Could not create private DeskLink data folder")?;
        ensure!(
            !fs::symlink_metadata(root)?.file_type().is_symlink(),
            "Device identity directory must not be a symbolic link"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        let path = root.join("identity.key");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                let mut key = [0; 32];
                OsRng.fill_bytes(&mut key);
                let write = (|| -> Result<()> {
                    file.write_all(b"DLI2")?;
                    file.write_all(&key)?;
                    file.sync_all()?;
                    Ok(())
                })();
                key.zeroize();
                write?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("Could not create device identity"),
        }
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 36, "Device identity is corrupt; restore its original file instead of silently changing this computer's identity");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                metadata.permissions().mode() & 0o077 == 0,
                "Device identity must be private to this user (0600)"
            );
        }
        let mut bytes = zeroize::Zeroizing::new([0; 36]);
        fs::File::open(path)?.read_exact(&mut bytes[..])?;
        ensure!(&bytes[..4] == b"DLI2", "Unsupported device identity format");
        let private: [u8; 32] = bytes[4..].try_into()?;
        ensure!(private != [0; 32], "Invalid device identity");
        let public = x25519_dalek::x25519(private, x25519_dalek::X25519_BASEPOINT_BYTES);
        Ok(Self {
            private,
            public,
            root: root.to_owned(),
        })
    }
    pub fn private_key(&self) -> &[u8; 32] {
        &self.private
    }
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public
    }
    fn state(&self) -> Result<Connection> {
        let path = self.root.join("devices.sqlite3");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            ensure!(
                !metadata.file_type().is_symlink(),
                "Device state must not be a symbolic link"
            );
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS peers (relay TEXT NOT NULL, id TEXT NOT NULL, key BLOB NOT NULL, PRIMARY KEY(relay,id)); CREATE TABLE IF NOT EXISTS own_ids (relay TEXT PRIMARY KEY, id TEXT NOT NULL);")?;
        Ok(connection)
    }
    pub fn check_peer(&self, relay: &str, id: &ComputerId, key: &[u8; 32]) -> Result<()> {
        let previous: Option<Vec<u8>> = self
            .state()?
            .query_row(
                "SELECT key FROM peers WHERE relay=?1 AND id=?2",
                params![relay, id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        ensure!(previous.is_none_or(|previous| previous == key), "This computer's identity changed. Connection blocked; verify the remote computer before resetting its remembered identity");
        Ok(())
    }
    pub fn remember_peer(&self, relay: &str, id: &ComputerId, key: &[u8; 32]) -> Result<()> {
        let mut state = self.state()?;
        let transaction =
            state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let previous: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT key FROM peers WHERE relay=?1 AND id=?2",
                params![relay, id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        ensure!(
            previous.is_none_or(|previous| previous == key),
            "Remembered computer identity changed"
        );
        transaction.execute(
            "INSERT OR IGNORE INTO peers (relay,id,key) VALUES (?1,?2,?3)",
            params![relay, id.as_str(), key.as_slice()],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn remember_own_id(&self, relay: &str, id: &ComputerId) -> Result<()> {
        if let Some(previous) = self.own_id(relay)? {
            ensure!(
                &previous == id,
                "The relay changed this computer's registered ID; restore its directory database"
            );
        }
        self.state()?.execute(
            "INSERT OR IGNORE INTO own_ids (relay,id) VALUES (?1,?2)",
            params![relay, id.as_str()],
        )?;
        Ok(())
    }
    pub fn own_id(&self, relay: &str) -> Result<Option<ComputerId>> {
        let id: Option<String> = self
            .state()?
            .query_row("SELECT id FROM own_ids WHERE relay=?1", [relay], |row| {
                row.get(0)
            })
            .optional()?;
        id.map(|id| ComputerId::parse(&id)).transpose()
    }
}
