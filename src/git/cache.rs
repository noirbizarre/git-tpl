//! Disposable source objects, never template state. See ADR-045.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{GitBackend, GitError, LibGit2, Oid};

/// Failures of disposable source storage and explicit network policy.
#[derive(Debug, Error, Diagnostic)]
pub enum CacheError {
    /// The cache could not be read or written.
    #[error("could not access Git cache at `{}`: {source}", path.display())]
    #[diagnostic(
        code(tpl::cache::io),
        help("Check cache permissions and free space, or choose --cache-dir.")
    )]
    Io {
        /// Affected cache path.
        path: PathBuf,
        /// Operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// Mutable names cannot establish a fresh offline resolution.
    #[error("offline resolution requires a cached full commit ID, not `{reference}`")]
    #[diagnostic(
        code(tpl::cache::offline_reference),
        help("Pass --ref with the full 40-character commit ID, or retry online.")
    )]
    OfflineReference {
        /// Name that needs refreshing.
        reference: String,
    },
    /// Required objects have not been cached completely.
    #[error("commit {reference} is not available in the cache for `{origin}`")]
    #[diagnostic(
        code(tpl::cache::offline_miss),
        help("Retry this exact commit online to populate the cache.")
    )]
    OfflineMiss {
        /// Credential-free source label.
        origin: String,
        /// Requested full commit ID.
        reference: String,
    },
    /// An explicitly network-capable operation conflicts with offline policy.
    #[error("{operation} is unavailable with --offline")]
    #[diagnostic(
        code(tpl::cache::offline),
        help("Retry without --offline to allow network access.")
    )]
    Offline {
        /// Rejected operation.
        operation: String,
    },
    /// No location was provided by the frontend.
    #[error("no cache directory is available")]
    #[diagnostic(
        code(tpl::cache::location),
        help("Set GIT_TPL_CACHE_DIR or pass --cache-dir with an absolute path.")
    )]
    Location,
    /// Disposable metadata is invalid.
    #[error("invalid Git cache metadata in `{}`", path.display())]
    #[diagnostic(
        code(tpl::cache::corrupt),
        help("Clear this cache entry and retry online.")
    )]
    Corrupt {
        /// Invalid entry metadata.
        path: PathBuf,
    },
    /// The caller required a tag or object pin, not a branch.
    #[error("`{reference}` is not a tag or unambiguous commit ID")]
    #[diagnostic(code(tpl::cache::unpinned), help("Use a tag or full commit ID."))]
    Unpinned {
        /// Requested pin.
        reference: String,
    },
    /// Fetching or reading Git objects failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] GitError),
}

fn io(path: &Path, source: std::io::Error) -> CacheError {
    CacheError::Io {
        path: path.into(),
        source,
    }
}

/// One invocation's source access policy and frozen symbolic resolutions.
#[derive(Debug, Clone)]
pub struct Access {
    root: Option<PathBuf>,
    /// Whether git-tpl may contact source repositories or HTTP data servers.
    pub offline: bool,
    resolutions: Rc<RefCell<Resolutions>>,
}

// Pin validation and general revspec lookup have different semantics, even for the same written name.
type Resolutions = BTreeMap<(String, Option<String>, bool), (String, Oid)>;

impl Access {
    /// Construct explicitly; environment discovery belongs to the frontend.
    pub fn new(root: Option<PathBuf>, offline: bool) -> Self {
        Self {
            root,
            offline,
            resolutions: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    /// Root of the versioned cache layout.
    pub fn root(&self) -> Result<&Path, CacheError> {
        self.root.as_deref().ok_or(CacheError::Location)
    }

    /// Reject an explicitly network-capable operation before it changes state.
    pub fn online(&self, operation: &str) -> Result<(), CacheError> {
        if self.offline {
            return Err(CacheError::Offline {
                operation: operation.into(),
            });
        }
        Ok(())
    }

    /// Open a shared source repository and freeze the requested ref under its mutation lock.
    pub fn acquire(
        &self,
        source: &str,
        reference: Option<&str>,
        pinned: bool,
    ) -> Result<Acquired, CacheError> {
        let key = (identity(source), reference.map(str::to_owned), pinned);
        let frozen = self.resolutions.borrow().get(&key).cloned();
        let exact = reference.and_then(Oid::parse);
        // A caller may clear an entry between acquisitions; reconstruct the frozen object, not today's name.
        let wanted = frozen.as_ref().map(|(_, oid)| *oid).or(exact);
        if self.offline && exact.is_none() {
            return Err(CacheError::OfflineReference {
                reference: reference.unwrap_or("<default branch>").into(),
            });
        }
        let root = self.root()?.join("v1");
        let entries = root.join("repos");
        let locks = root.join("locks");
        fs::create_dir_all(&entries).map_err(|e| io(&entries, e))?;
        fs::create_dir_all(&locks).map_err(|e| io(&locks, e))?;
        #[cfg(unix)]
        private_directory(&root)?;
        let id = key.0.clone();
        let lease_path = locks.join(format!("{id}.lease"));
        let lease = Lock(lock_file(&lease_path)?);
        lease.0.lock_shared().map_err(|e| io(&lease_path, e))?;
        let mutation_path = locks.join(format!("{id}.fetch"));
        let mutation = Lock(lock_file(&mutation_path)?);
        mutation.0.lock().map_err(|e| io(&mutation_path, e))?;
        let staging_prefix = format!(".staging-{id}-");
        let path = entries.join(&id);
        let miss = || CacheError::OfflineMiss {
            origin: redact(source),
            reference: reference.unwrap_or("<default branch>").into(),
        };
        let created = !path.exists();
        if created {
            if self.offline {
                return Err(miss());
            }
            // Only this entry's fetch holder can own a matching abandoned staging directory.
            // Do not scan all repository entries on warm cache hits.
            for entry in fs::read_dir(&entries).map_err(|e| io(&entries, e))? {
                let entry = entry.map_err(|e| io(&entries, e))?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&staging_prefix)
                {
                    fs::remove_dir_all(entry.path()).map_err(|e| io(&entry.path(), e))?;
                }
            }
            let staging = tempfile::Builder::new()
                .prefix(&staging_prefix)
                .tempdir_in(&entries)
                .map_err(|e| io(&entries, e))?;
            let repo_path = staging.path().join("repo");
            let repo = LibGit2::init_source_cache(&repo_path)?;
            repo.refresh_source(source, wanted)?;
            write_metadata(
                &repo_path,
                &Metadata {
                    source: redact(source),
                    last_used: now(),
                },
            )?;
            drop(repo);
            fs::rename(&repo_path, &path).map_err(|e| io(&path, e))?;
        }
        let repo = LibGit2::open(&path).map_err(|_| CacheError::Corrupt { path: path.clone() })?;
        let available = wanted.is_some_and(|oid| repo.complete_commit(oid).unwrap_or(false));
        if !created && !available {
            if self.offline {
                return Err(miss());
            }
            repo.refresh_source(source, wanted)?;
        }
        let (reference, revision) = match frozen {
            Some(pair) => pair,
            None => {
                let reference = reference
                    .map(str::to_owned)
                    .map(Ok)
                    .unwrap_or_else(|| repo.default_branch())?;
                let revision = if let Some(oid) = exact {
                    // A full ID names an object, never a coincidentally hexadecimal tag.
                    repo.commit_tree(oid)?;
                    oid
                } else if pinned {
                    repo.resolve_pin(&reference, source)?
                        .ok_or_else(|| CacheError::Unpinned {
                            reference: reference.clone(),
                        })?
                } else {
                    repo.resolve_revision(&reference, source)?
                };
                (reference, revision)
            }
        };
        if !available && !repo.complete_commit(revision)? {
            return Err(if self.offline {
                miss()
            } else {
                CacheError::Corrupt { path: path.clone() }
            });
        }
        self.resolutions
            .borrow_mut()
            .insert(key, (reference.clone(), revision));
        let metadata = Metadata {
            source: redact(source),
            last_used: now(),
        };
        write_metadata(&path, &metadata)?;
        drop(mutation);
        Ok(Acquired {
            repo,
            reference,
            revision,
            lease: Lease(Arc::new(lease)),
        })
    }

    /// Explicitly refresh all advertised heads/tags, even after earlier acquisition.
    pub fn fetch(&self, source: &str) -> Result<(), CacheError> {
        self.online("cache fetch")?;
        let fresh = Self::new(self.root.clone(), false);
        fresh.acquire(source, None, false)?;
        Ok(())
    }

    /// Inspect entries without creating a cache on disk.
    pub fn list(&self) -> Result<Vec<Entry>, CacheError> {
        let mut entries = Vec::new();
        for id in self.ids()? {
            let path = self.root()?.join("v1/repos").join(&id);
            let lease_path = self.root()?.join("v1/locks").join(format!("{id}.lease"));
            let lease = Lock(lock_file(&lease_path)?);
            lease.0.lock_shared().map_err(|e| io(&lease_path, e))?;
            let metadata_path = path.join("git-tpl-cache.json");
            let bytes = match fs::read(&metadata_path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !path.exists() => continue,
                Err(e) => return Err(io(&metadata_path, e)),
            };
            let metadata: Metadata =
                serde_json::from_slice(&bytes).map_err(|_| CacheError::Corrupt {
                    path: metadata_path,
                })?;
            entries.push(Entry {
                id,
                source: metadata.source,
                last_used: metadata.last_used,
                bytes: directory_size(&path)?,
            });
        }
        Ok(entries)
    }

    fn ids(&self) -> Result<Vec<String>, CacheError> {
        let path = self.root()?.join("v1/repos");
        let dirs = match fs::read_dir(&path) {
            Ok(dirs) => dirs,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&path, e)),
        };
        let mut ids = Vec::new();
        for dir in dirs {
            let dir = dir.map_err(|e| io(&path, e))?;
            let id = dir.file_name().to_string_lossy().into_owned();
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            ids.push(id);
        }
        ids.sort();
        Ok(ids)
    }

    /// Remove whole unused entries; busy entries are reported rather than waited on.
    pub fn clear(
        &self,
        source: Option<&str>,
        older_than: Option<Duration>,
    ) -> Result<Removal, CacheError> {
        let mut result = Removal::default();
        let selected = source.map(identity);
        for id in self.ids()? {
            if selected.as_ref().is_some_and(|selected| *selected != id) {
                continue;
            }
            let path = self.root()?.join("v1/repos").join(&id);
            let lease_path = self.root()?.join("v1/locks").join(format!("{id}.lease"));
            let lease = Lock(lock_file(&lease_path)?);
            match lease.0.try_lock() {
                Ok(()) => (),
                Err(std::fs::TryLockError::WouldBlock) => {
                    result.skipped.push(id);
                    continue;
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(io(&lease_path, e)),
            }
            if !path.exists() {
                continue;
            }
            // Recheck while exclusively leased: a renderer may have touched it since list().
            if let Some(age) = older_than {
                let metadata_path = path.join("git-tpl-cache.json");
                let metadata: Metadata = serde_json::from_slice(
                    &fs::read(&metadata_path).map_err(|e| io(&metadata_path, e))?,
                )
                .map_err(|_| CacheError::Corrupt {
                    path: metadata_path,
                })?;
                if now().saturating_sub(metadata.last_used) < age.as_secs() {
                    continue;
                }
            }
            fs::remove_dir_all(&path).map_err(|e| io(&path, e))?;
            result.removed.push(id);
        }
        Ok(result)
    }
}

/// Source objects and their lifetime lease. The handle drops first on Windows too.
pub struct Acquired {
    /// Handle drops before the protecting lease.
    pub repo: LibGit2,
    /// Requested name, or the freshly advertised default branch.
    pub reference: String,
    /// Commit frozen under the mutation lock.
    pub revision: Oid,
    /// Prevents entry removal while source objects are being read.
    pub lease: Lease,
}

/// Shared file lock; never unlink its inode while other processes can be waiting.
#[derive(Clone, Debug)]
pub struct Lease(#[allow(dead_code)] Arc<Lock>);

#[derive(Debug)]
struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        // Explicit unlock also releases copies inherited by a concurrent fork before exec closes them.
        // Closing our descriptor alone can leave maintenance spuriously busy until the child execs.
        let _ = self.0.unlock();
    }
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    source: String,
    last_used: u64,
}

/// A disposable repository's inspection data.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Opaque repository identity.
    pub id: String,
    /// Source label without URL credentials.
    pub source: String,
    /// Last successful access, in Unix seconds.
    pub last_used: u64,
    /// Current on-disk file sizes.
    pub bytes: u64,
}

/// Maintenance result, including active readers left untouched.
#[derive(Debug, Default, Serialize)]
pub struct Removal {
    /// IDs of removed repositories.
    pub removed: Vec<String>,
    /// IDs retained because another operation is using them.
    pub skipped: Vec<String>,
}

/// Conservative identity: different transports/accounts/paths cannot collide.
pub fn identity(source: &str) -> String {
    let canonical = crate::refs::local_path(source).and_then(|path| path.canonicalize().ok());
    let canonical = canonical.as_ref().map(|path| path.to_string_lossy());
    hex::encode(Sha256::digest(
        canonical.as_deref().unwrap_or(source).as_bytes(),
    ))
}

/// Persistent labels never retain URL credentials, queries, or fragments.
pub fn redact(source: &str) -> String {
    if let Some((scheme, rest)) = source.split_once("://") {
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        return format!("{scheme}://{host}/{path}");
    }
    source.into()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn lock_file(path: &Path) -> Result<File, CacheError> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| io(path, e))
}

#[cfg(unix)]
fn private_directory(path: &Path) -> Result<(), CacheError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| io(path, e))
}

fn write_metadata(path: &Path, metadata: &Metadata) -> Result<(), CacheError> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path).map_err(|e| io(path, e))?;
    file.write_all(&serde_json::to_vec(metadata).expect("metadata is serializable"))
        .map_err(|e| io(path, e))?;
    file.persist(path.join("git-tpl-cache.json"))
        .map_err(|e| io(path, e.error))?;
    Ok(())
}

fn directory_size(path: &Path) -> Result<u64, CacheError> {
    let mut size = 0;
    for entry in fs::read_dir(path).map_err(|e| io(path, e))? {
        let entry = entry.map_err(|e| io(path, e))?;
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(&entry.path(), e)),
        };
        if metadata.is_dir() {
            size += directory_size(&entry.path())?;
        } else {
            size += metadata.len();
        }
    }
    Ok(size)
}
