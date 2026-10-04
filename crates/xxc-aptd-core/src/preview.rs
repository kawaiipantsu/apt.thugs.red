//! Publication review tied to the exact active generation and selected content.
use crate::{config::Config, db::Database, package::Package, repository};
use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub struct VersionChange {
    pub name: String,
    pub architecture: String,
    pub before: String,
    pub after: String,
}
#[derive(Serialize)]
pub struct Preview {
    pub token: String,
    pub suite: String,
    pub current_generation: Option<String>,
    pub added: Vec<Package>,
    pub removed: Vec<Package>,
    pub upgrades: Vec<VersionChange>,
    pub downgrades: Vec<VersionChange>,
    pub architectures_added: Vec<String>,
    pub architectures_removed: Vec<String>,
    pub size_delta: i128,
}
pub fn preview(c: &Config, db: &Database) -> Result<Preview> {
    let current = repository::current(c)?;
    let before = current
        .as_ref()
        .map(|m| m.suite_packages(db.suite()))
        .unwrap_or(&[]);
    let after = db.selected()?;
    let current_generation = current.as_ref().map(|m| m.id.clone());
    let token = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            db.suite(),
            &current_generation,
            &after,
            &c.repository,
            &c.signing,
            &c.xxc_trust
        ))?)
    );
    let before_ids: BTreeSet<_> = before.iter().map(|p| &p.id).collect();
    let after_ids: BTreeSet<_> = after.iter().map(|p| &p.id).collect();
    let added: Vec<_> = after
        .iter()
        .filter(|p| !before_ids.contains(&p.id))
        .cloned()
        .collect();
    let removed = before
        .iter()
        .filter(|p| !after_ids.contains(&p.id))
        .cloned()
        .collect();
    let mut latest = BTreeMap::new();
    for p in before {
        let version = p.version.parse::<debversion::Version>()?;
        let entry = latest
            .entry((&p.name, &p.architecture))
            .or_insert((version.clone(), &p.version));
        if version > entry.0 {
            *entry = (version, &p.version);
        }
    }
    let mut upgrades = vec![];
    let mut downgrades = vec![];
    for p in &added {
        if let Some((old, text)) = latest.get(&(&p.name, &p.architecture)) {
            let v = p.version.parse::<debversion::Version>()?;
            let change = VersionChange {
                name: p.name.clone(),
                architecture: p.architecture.clone(),
                before: (*text).clone(),
                after: p.version.clone(),
            };
            if v > *old {
                upgrades.push(change);
            } else if v < *old {
                downgrades.push(change);
            }
        }
    }
    let a: BTreeSet<_> = before.iter().map(|p| p.architecture.clone()).collect();
    let b: BTreeSet<_> = after.iter().map(|p| p.architecture.clone()).collect();
    Ok(Preview {
        token,
        suite: db.suite().into(),
        current_generation,
        added,
        removed,
        upgrades,
        downgrades,
        architectures_added: b.difference(&a).cloned().collect(),
        architectures_removed: a.difference(&b).cloned().collect(),
        size_delta: after.iter().map(|p| i128::from(p.size)).sum::<i128>()
            - before.iter().map(|p| i128::from(p.size)).sum::<i128>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_tracks_selection_config_and_debian_versions() {
        let root = tempfile::tempdir().unwrap();
        let c = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let db = Database::open(&c).unwrap();
        let old = Package {
            id: uuid::Uuid::new_v4().to_string(),
            name: "fixture".into(),
            version: "1:2.0-1".into(),
            architecture: "amd64".into(),
            source: "fixture".into(),
            component: "main".into(),
            description: String::new(),
            fields: Default::default(),
            filename: "pool/main/f/fixture/fixture.deb".into(),
            size: 12,
            sha256: "a".repeat(64),
            sha512: String::new(),
            uploaded: repository::now(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let dir = c.paths.repository.join(".generations").join(&id);
        std::fs::create_dir_all(dir.join("dists")).unwrap();
        let manifest = repository::Manifest {
            id: id.clone(),
            created: repository::now(),
            suite: c.repository.suite.clone(),
            fingerprint: String::new(),
            packages: vec![old.clone()],
            suites: Default::default(),
            files: Default::default(),
        };
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            format!(".generations/{id}/dists"),
            c.paths.repository.join("dists"),
        )
        .unwrap();
        db.insert(&old).unwrap();
        db.connect()
            .unwrap()
            .execute("UPDATE package_suites SET active=1", [])
            .unwrap();
        let initial = preview(&c, &db).unwrap();
        assert!(initial.added.is_empty());
        let mut newer = old.clone();
        newer.id = uuid::Uuid::new_v4().to_string();
        newer.version = "2:1.0~rc1-1".into();
        newer.size = 20;
        db.insert(&newer).unwrap();
        assert_eq!(initial.token, preview(&c, &db).unwrap().token);
        db.stage(&newer.id).unwrap();
        let after = preview(&c, &db).unwrap();
        assert_ne!(initial.token, after.token);
        assert_eq!(after.upgrades.len(), 1);
        assert_eq!(after.size_delta, 20);
        let mut older = old.clone();
        older.id = uuid::Uuid::new_v4().to_string();
        older.version = "1:2.0~rc1-1".into();
        db.insert(&older).unwrap();
        db.stage(&older.id).unwrap();
        assert_eq!(preview(&c, &db).unwrap().downgrades.len(), 1);
        let mut changed = c.clone();
        changed.repository.description = "new description".into();
        assert_ne!(
            preview(&c, &db).unwrap().token,
            preview(&changed, &db).unwrap().token
        );
    }
}
