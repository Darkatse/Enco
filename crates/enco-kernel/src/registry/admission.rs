use super::*;

/// Whether each registered name's active generation, with any candidates substituted, can run on
/// this node, and why not. It depends only on the generations' own facts, never on their users.
#[derive(Default)]
pub(super) struct Admission(BTreeMap<String, Result<Loaded, Failure>>);

pub(super) fn validate_name(name: &str) -> Result<(), RegistryError> {
    if name.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    }) {
        Ok(())
    } else {
        Err(rejected(
            name,
            "name must use lowercase letters, digits and single hyphens",
        ))
    }
}

pub(super) fn users(uses: &[Use], name: &str) -> Vec<String> {
    uses.iter()
        .filter(|usage| usage.endpoint.plugin == name)
        .map(|usage| usage.user.clone())
        .collect()
}

/// Evaluate one complete proposed composition, independent of how it was assembled.
pub(super) fn evaluate(
    contents: BTreeMap<String, Option<Loaded>>,
    host_tool_names: &[String],
) -> Admission {
    Admission(check(contents, host_tool_names))
}

fn check(
    contents: BTreeMap<String, Option<Loaded>>,
    native: &[String],
) -> BTreeMap<String, Result<Loaded, Failure>> {
    let mut reasons: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tool_owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, loaded) in &contents {
        let reasons = reasons.entry(name.clone()).or_default();
        let Some(loaded) = loaded else {
            reasons.push("no active generation".into());
            continue;
        };
        if let Some(failure) = &loaded.unavailable {
            reasons.push(format!("{}: {}", failure.code, failure.message));
        }
        if let Some(tools) = &loaded.tools {
            for spec in &tools.specs {
                tool_owners.entry(&spec.name).or_default().push(name);
            }
        }
    }
    for (tool, owners) in tool_owners {
        let conflict = if native.iter().any(|name| name == tool) {
            Some(format!("tool {tool} conflicts with a native tool"))
        } else if owners.len() > 1 {
            Some(format!(
                "tool {tool} is exported by conflicting plugins: {}",
                owners.join(", ")
            ))
        } else {
            None
        };
        if let Some(conflict) = conflict {
            for owner in owners {
                reasons
                    .entry(owner.into())
                    .or_default()
                    .push(conflict.clone());
            }
        }
    }
    contents
        .into_iter()
        .map(|(name, loaded)| {
            let reasons = &reasons[&name];
            let result = match loaded {
                Some(loaded) if reasons.is_empty() => Ok(loaded),
                _ => Err(Failure {
                    code: code::PLUGIN_UNAVAILABLE.into(),
                    message: reasons.join("; "),
                    retryable: false,
                }),
            };
            (name, result)
        })
        .collect()
}

impl Admission {
    /// The admitted code behind a name, or why it cannot run.
    pub fn get(&self, name: &str) -> Result<&Loaded, Failure> {
        match self.0.get(name) {
            Some(result) => result.as_ref().map_err(Clone::clone),
            None => Err(exports::unavailable(name, "name is not registered")),
        }
    }

    /// An intentional change must leave its candidates and every new or changed use working,
    /// and must not stop anything that worked before. Failures that predate the change and that
    /// it does not touch stay reported without blocking it, so repairs can land one at a time.
    pub fn accept_change(
        &self,
        before: &Self,
        candidates: &[String],
        uses: &[Use],
        previous: &[Use],
    ) -> Result<(), RegistryError> {
        for (name, result) in &self.0 {
            if let Err(failure) = result
                && (candidates.contains(name) || before.get(name).is_ok())
            {
                return Err(rejected(name, &failure.message));
            }
        }
        for usage in uses {
            let changed = !previous
                .iter()
                .any(|old| old.user == usage.user && old.same_call(usage));
            let available = |admission: &Self| usage.check(admission.get(&usage.endpoint.plugin)?);
            if let Err(failure) = available(self)
                && (changed || available(before).is_ok())
            {
                return Err(rejected(
                    &usage.endpoint.plugin,
                    format!("{}: {}", usage.user, failure.message),
                ));
            }
        }
        Ok(())
    }
}
