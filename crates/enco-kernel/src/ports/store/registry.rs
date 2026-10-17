//! Durable registry facts and its two kinds of atomic writes.
use enco_core::*;
use std::collections::BTreeMap;

/// Durable facts; compiled code and admission are projections of this history.
#[derive(Default, Clone)]
pub struct RegistryState {
    /// Accepted changes in structural order.
    pub registrations: BTreeMap<RegistrationId, RegistrationRecord>,
    /// Every installation, including stopped and removed identities.
    pub plugins: BTreeMap<PluginId, PluginRecord>,
    /// Immutable plugin content and its observed health.
    pub generations: BTreeMap<GenerationId, GenerationRecord>,
    /// Immutable configuration content.
    pub config_generations: BTreeMap<GenerationId, ConfigGeneration>,
    /// Current configuration, absent only before initial registration.
    pub active_config: Option<GenerationId>,
}

/// The sole committer assigns identities before handing a complete write to the store.
#[derive(Clone)]
pub struct RegistrationWrite {
    /// Accepted change, including the previous name references.
    pub registration: RegistrationRecord,
    /// Plugin generations created in this change.
    pub generations: Vec<GenerationRecord>,
    /// A configuration to create and activate, when its intent changed.
    pub configuration: Option<ConfigGeneration>,
    /// Final runtime references for each affected identity.
    pub plugins: Vec<PluginRecord>,
}

/// Mechanical recovery moves references; it never creates new intent.
#[derive(Default, Clone)]
pub struct ActivationWrite {
    /// Final runtime references for affected identities.
    pub plugins: Vec<PluginRecord>,
    /// Configuration to activate, or leave unchanged when absent.
    pub configuration: Option<GenerationId>,
    /// Attributable failures observed while recovering.
    pub failed: Vec<(GenerationId, Failure)>,
    /// Optional Session input committed with automatic recovery.
    pub event: Option<Event>,
}
