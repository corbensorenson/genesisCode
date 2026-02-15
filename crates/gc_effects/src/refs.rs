use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};

use crate::error::EffectsError;

#[derive(Debug, Clone)]
pub struct RefsDb {
    path: PathBuf,
    lock_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RefEntry {
    pub name: String,
    pub hash: Option<String>,
}

#[derive(Debug)]
pub enum SetResult {
    Updated,
    Conflict { current: Option<String> },
}

impl RefsDb {
    pub fn open(path: &Path) -> Result<Self, EffectsError> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let lock_path = path.with_extension("lock");
        Ok(Self {
            path: path.to_path_buf(),
            lock_path,
        })
    }

    pub fn get(&self, name: &str) -> Result<Option<String>, EffectsError> {
        let _lk = self.lock_exclusive()?;
        let db = self.load_locked()?;
        Ok(db.get(name).cloned())
    }

    pub fn list(&self, prefix: Option<&str>) -> Result<Vec<RefEntry>, EffectsError> {
        let _lk = self.lock_exclusive()?;
        let db = self.load_locked()?;
        let mut out = Vec::new();
        for (k, v) in db {
            if let Some(p) = prefix
                && !k.starts_with(p)
            {
                continue;
            }
            out.push(RefEntry {
                name: k,
                hash: Some(v),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn set(
        &self,
        name: &str,
        new_hash: Option<&str>,
        expected_old: Option<Option<&str>>,
    ) -> Result<SetResult, EffectsError> {
        let _lk = self.lock_exclusive()?;
        let mut db = self.load_locked()?;

        let cur = db.get(name).map(|s| s.as_str());
        if let Some(exp) = expected_old {
            match (exp, cur) {
                (None, None) => {}
                (Some(e), Some(c)) if e == c => {}
                _ => {
                    return Ok(SetResult::Conflict {
                        current: cur.map(|s| s.to_string()),
                    });
                }
            }
        }

        match new_hash {
            Some(h) => {
                db.insert(name.to_string(), h.to_string());
            }
            None => {
                db.remove(name);
            }
        }

        self.write_locked(&db)?;
        Ok(SetResult::Updated)
    }

    fn lock_exclusive(&self) -> Result<std::fs::File, EffectsError> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)?;
        f.lock_exclusive()?;
        Ok(f)
    }

    fn load_locked(&self) -> Result<BTreeMap<String, String>, EffectsError> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let s = std::fs::read_to_string(&self.path)?;
        let t = parse_term(&s).map_err(|e| EffectsError::Log(format!("refs db parse: {e}")))?;
        let Term::Map(m) = t else {
            return Err(EffectsError::Log("refs db: expected map".to_string()));
        };
        let v = m.get(&TermOrdKey(Term::symbol(":v")));
        if !matches!(v, Some(Term::Int(i)) if i == &1.into()) {
            return Err(EffectsError::Log(
                "refs db: wrong or missing :v".to_string(),
            ));
        }
        let kind = m.get(&TermOrdKey(Term::symbol(":kind")));
        if !matches!(kind, Some(Term::Str(s)) if s == "genesis/refs-db-v0.1") {
            return Err(EffectsError::Log(
                "refs db: wrong or missing :kind".to_string(),
            ));
        }
        let Some(Term::Map(refs)) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
            return Err(EffectsError::Log("refs db: missing :refs map".to_string()));
        };

        let mut out = BTreeMap::new();
        for (k, v) in refs {
            let Term::Str(name) = &k.0 else {
                return Err(EffectsError::Log(
                    "refs db: :refs keys must be strings".to_string(),
                ));
            };
            let Term::Str(hex) = v else {
                return Err(EffectsError::Log(
                    "refs db: :refs values must be strings".to_string(),
                ));
            };
            out.insert(name.clone(), hex.clone());
        }
        Ok(out)
    }

    fn write_locked(&self, db: &BTreeMap<String, String>) -> Result<(), EffectsError> {
        let mut refs = BTreeMap::new();
        for (k, v) in db {
            refs.insert(TermOrdKey(Term::Str(k.clone())), Term::Str(v.clone()));
        }
        let t = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/refs-db-v0.1".to_string()),
                ),
                (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                (TermOrdKey(Term::symbol(":refs")), Term::Map(refs)),
            ]
            .into_iter()
            .collect(),
        );
        let s = print_term(&t) + "\n";

        let tmp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(s.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &self.path)?;
        #[cfg(unix)]
        {
            if let Some(parent) = self.path.parent() {
                let dir = std::fs::File::open(parent)?;
                dir.sync_all()?;
            }
        }
        Ok(())
    }
}
