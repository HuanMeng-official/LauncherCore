//! Named game instances backed by independent `.minecraft`-style directories.

use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceEntry {
    pub path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct InstanceRegistry {
    #[serde(default)]
    instances: BTreeMap<String, InstanceEntry>,
}

pub struct InstanceStore {
    registry_path: PathBuf,
    instances_root: PathBuf,
}

impl InstanceStore {
    pub fn new() -> anyhow::Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow!("Could not determine config directory"))?
            .join("mclc");
        let instances_root = dirs::data_local_dir()
            .ok_or_else(|| anyhow!("Could not determine local data directory"))?
            .join("mclc")
            .join("instances");
        Ok(Self {
            registry_path: config_dir.join("instances.json"),
            instances_root,
        })
    }

    pub fn list(&self) -> anyhow::Result<BTreeMap<String, InstanceEntry>> {
        Ok(self.load()?.instances)
    }

    pub fn resolve(&self, name: &str) -> anyhow::Result<PathBuf> {
        let registry = self.load()?;
        if let Some(entry) = registry.instances.get(name) {
            if !entry.path.is_dir() {
                bail!(
                    "Instance '{}' directory is missing: {}",
                    name,
                    entry.path.display()
                );
            }
            return Ok(entry.path.clone());
        }

        let available = registry
            .instances
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if available.is_empty() {
            bail!(
                "Instance '{}' not found. Create it with 'mclc instance create {}'.",
                name,
                name
            );
        }
        bail!(
            "Instance '{}' not found. Available instances: {}",
            name,
            available
        )
    }

    pub fn create(&self, name: &str, path: Option<&Path>) -> anyhow::Result<InstanceEntry> {
        validate_name(name)?;
        let mut registry = self.load()?;
        if registry.instances.contains_key(name) {
            bail!("Instance '{}' already exists", name);
        }

        let path = match path {
            Some(path) => absolute_path(path)?,
            None => self.instances_root.join(name),
        };
        if registry.instances.values().any(|entry| entry.path == path) {
            bail!("The directory {} is already registered", path.display());
        }
        if path.exists() && !path.is_dir() {
            bail!("{} is not a directory", path.display());
        }

        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create instance directory {}", path.display()))?;
        let entry = InstanceEntry { path };
        registry.instances.insert(name.to_string(), entry.clone());
        self.save(&registry)?;
        Ok(entry)
    }

    pub fn delete(&self, name: &str, delete_files: bool) -> anyhow::Result<InstanceEntry> {
        let mut registry = self.load()?;
        let entry = registry
            .instances
            .remove(name)
            .ok_or_else(|| anyhow!("Instance '{}' not found", name))?;

        if delete_files && entry.path.exists() {
            if entry.path.parent().is_none() {
                bail!(
                    "Refusing to delete filesystem root {}",
                    entry.path.display()
                );
            }
            if let Ok(current_dir) = std::env::current_dir()
                && current_dir.starts_with(&entry.path)
            {
                bail!(
                    "Refusing to delete {} because it contains the current directory",
                    entry.path.display()
                );
            }
            fs::remove_dir_all(&entry.path).with_context(|| {
                format!(
                    "Failed to delete instance directory {}",
                    entry.path.display()
                )
            })?;
        }

        self.save(&registry)?;
        Ok(entry)
    }

    pub fn instances_root(&self) -> &Path {
        &self.instances_root
    }

    fn load(&self) -> anyhow::Result<InstanceRegistry> {
        if !self.registry_path.exists() {
            return Ok(InstanceRegistry::default());
        }
        let text = fs::read_to_string(&self.registry_path)
            .with_context(|| format!("Failed to read {}", self.registry_path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse {}", self.registry_path.display()))
    }

    fn save(&self, registry: &InstanceRegistry) -> anyhow::Result<()> {
        if let Some(parent) = self.registry_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(registry)?;
        fs::write(&self.registry_path, text)
            .with_context(|| format!("Failed to write {}", self.registry_path.display()))
    }

    #[cfg(test)]
    fn with_paths(registry_path: PathBuf, instances_root: PathBuf) -> Self {
        Self {
            registry_path,
            instances_root,
        }
    }
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        bail!("Instance name cannot be empty");
    }
    if name.trim() != name {
        bail!("Instance name cannot start or end with whitespace");
    }
    if name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        bail!("Instance name contains invalid path characters");
    }
    if name.chars().any(|character| {
        character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
    }) {
        bail!("Instance name contains invalid characters");
    }
    Ok(())
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, InstanceStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = InstanceStore::with_paths(
            temp.path().join("config/instances.json"),
            temp.path().join("instances"),
        );
        (temp, store)
    }

    #[test]
    fn creates_lists_and_resolves_instances() {
        let (_temp, store) = store();

        let entry = store.create("fabric", None).unwrap();
        assert_eq!(entry.path, store.instances_root().join("fabric"));
        assert!(entry.path.is_dir());

        let instances = store.list().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances["fabric"].path, entry.path);
        assert_eq!(store.resolve("fabric").unwrap(), entry.path);
    }

    #[test]
    fn supports_custom_instance_paths() {
        let (temp, store) = store();
        let custom = temp.path().join("custom-game");
        let entry = store.create("custom", Some(&custom)).unwrap();
        assert_eq!(entry.path, custom);
        assert!(custom.is_dir());
    }

    #[test]
    fn rejects_duplicate_names_and_paths() {
        let (_temp, store) = store();
        store.create("one", None).unwrap();
        assert!(store.create("one", None).is_err());
        assert!(
            store
                .create("two", Some(&store.instances_root().join("one")))
                .is_err()
        );
    }

    #[test]
    fn deletes_registry_but_keeps_files_by_default() {
        let (_temp, store) = store();
        let entry = store.create("keep", None).unwrap();

        let removed = store.delete("keep", false).unwrap();
        assert_eq!(removed.path, entry.path);
        assert!(entry.path.exists());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn deletes_files_when_requested() {
        let (_temp, store) = store();
        let entry = store.create("remove", None).unwrap();

        store.delete("remove", true).unwrap();
        assert!(!entry.path.exists());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_names() {
        let (_temp, store) = store();
        for name in [
            "",
            " leading",
            "trailing ",
            ".",
            "..",
            "a/b",
            "a\\b",
            "bad:name",
        ] {
            assert!(
                store.create(name, None).is_err(),
                "{name:?} must be rejected"
            );
        }
    }
}
