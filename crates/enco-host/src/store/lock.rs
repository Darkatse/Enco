use enco_core::PluginId;
use enco_kernel::StoreError;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginNames {
    plugins: BTreeMap<String, Registration>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    id: PluginId,
}

fn failure(path: &Path, error: impl std::fmt::Display) -> StoreError {
    StoreError::Lock(format!("{}: {error}", path.display()))
}

pub(super) fn read(path: &Path) -> Result<BTreeMap<String, PluginId>, StoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(failure(path, error)),
    };
    let names: PluginNames = toml::from_str(&text).map_err(|error| failure(path, error))?;
    let mut identities = BTreeSet::new();
    let mut result = BTreeMap::new();
    for (name, registration) in names.plugins {
        if !identities.insert(registration.id) {
            return Err(failure(
                path,
                "one identity is registered under multiple names",
            ));
        }
        result.insert(name, registration.id);
    }
    Ok(result)
}

/// Called under the Store's lock on a blocking thread, before committing registry rows.
pub(super) fn register(path: &Path, name: String, id: PluginId) -> Result<(), StoreError> {
    let mut names = read(path)?;
    if names.contains_key(&name) || names.values().any(|existing| *existing == id) {
        return Err(failure(
            path,
            format!("name or identity is already registered: {name}"),
        ));
    }
    names.insert(name, id);
    let body = PluginNames {
        plugins: names
            .into_iter()
            .map(|(name, id)| (name, Registration { id }))
            .collect(),
    };
    let text = format!(
        "# Maintained by enco; do not edit by hand.\n{}",
        toml::to_string(&body).map_err(|error| failure(path, error))?
    );
    let temp = path.with_extension(format!("tmp.{}", ulid::Ulid::generate()));
    let mut file = std::fs::File::create(&temp).map_err(|error| failure(path, error))?;
    file.write_all(text.as_bytes())
        .map_err(|error| failure(path, error))?;
    file.sync_all().map_err(|error| failure(path, error))?;
    std::fs::rename(temp, path).map_err(|error| failure(path, error))
}
