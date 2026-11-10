//! Durable registry facts, split into accepted intent and this node's references, and the two
//! kinds of atomic writes.
use enco_core::*;
use std::collections::BTreeMap;

/// Durable facts; compiled code and admission are projections of them.
#[derive(Default, Clone)]
pub struct RegistryState {
    /// What was registered: the same on every node.
    pub intent: AcceptedIntent,
    /// What runs on this node and how it fared.
    pub node: NodeState,
}

/// Accepted changes and the content they brought; identities derive from registrations, so
/// every node agrees on them.
#[derive(Default, Clone)]
pub struct AcceptedIntent {
    /// Accepted changes in structural order.
    pub registrations: BTreeMap<RegistrationId, RegistrationRecord>,
    /// Immutable plugin content, by the registration that brought it.
    pub generations: BTreeMap<GenerationId, GenerationRecord>,
    /// Immutable configuration content, by the registration that brought it.
    pub configurations: BTreeMap<RegistrationId, ConfigGeneration>,
}

/// This node's references and observations; another node keeps its own.
#[derive(Default, Clone)]
pub struct NodeState {
    /// Each plugin's name, current code and rollback target on this node.
    pub plugins: BTreeMap<PluginId, PluginRecord>,
    /// Each generation's trial, health or failure on this node.
    pub health: BTreeMap<GenerationId, Health>,
    /// Registration of the configuration that runs, absent only before initial registration.
    pub configuration: Option<RegistrationId>,
    /// What each registration replaced on this node, read by the owner's rollback.
    pub replaced: BTreeMap<RegistrationId, Replaced>,
}

/// The references one registration replaced on this node, so the owner can restore them here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Replaced {
    /// For each plugin whose name it wrote: the registration whose name ran before, absent for a
    /// plugin that had none.
    pub names: BTreeMap<PluginId, Option<RegistrationId>>,
    /// The configuration that ran before, when it activated one.
    pub configuration: Option<RegistrationId>,
}

/// The sole committer derives identities before handing a complete write to the store.
#[derive(Clone)]
pub struct RegistrationWrite {
    /// Accepted change.
    pub registration: RegistrationRecord,
    /// Plugin generations brought by this change; each starts on trial on this node.
    pub generations: Vec<GenerationRecord>,
    /// A configuration to create and activate, when its intent changed.
    pub configuration: Option<ConfigGeneration>,
    /// Final references on this node for each affected identity.
    pub plugins: Vec<PluginRecord>,
    /// What this change replaced on this node; the store checks it still runs.
    pub replaced: Replaced,
}

/// Mechanical recovery moves this node's references; it never creates new intent.
#[derive(Default, Clone)]
pub struct ActivationWrite {
    /// Final references for affected identities.
    pub plugins: Vec<PluginRecord>,
    /// Configuration to activate, or leave unchanged when absent.
    pub configuration: Option<RegistrationId>,
    /// Attributable failures observed while recovering.
    pub failed: Vec<(GenerationId, Failure)>,
    /// Optional Session input committed with automatic recovery.
    pub event: Option<Event>,
}
