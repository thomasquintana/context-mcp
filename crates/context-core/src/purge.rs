//! Transitive, derivation-aware purge planning without soft-deleted content.

use crate::{content::Relationship, ids::ContentId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Directed dependency from a source body to a dependent managed body.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// Source managed content.
    pub source: ContentId,
    /// Dependent managed content.
    pub dependent: ContentId,
    /// Derivation propagates purge; citation-only dependence invalidates support.
    pub relationship: Relationship,
}

/// Content-free deletion worklist. Compute under a transaction blocking new dependencies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgePlan {
    /// Bodies/index entries/objects to remove, including all derivation descendants.
    pub delete: BTreeSet<ContentId>,
    /// Independent surviving bodies whose supporting citations become unavailable.
    pub invalidate_support: BTreeSet<ContentId>,
}

impl PurgePlan {
    /// Build the complete closure, terminating safely even with corrupt cyclic lineage.
    ///
    /// A mixed-source descendant is deleted in full. No automatic regeneration is
    /// implied. Callers must load every edge across revisions and sharing boundaries,
    /// fence publication, and persist cleanup work before releasing the transaction.
    pub fn from_dependencies(
        roots: impl IntoIterator<Item = ContentId>,
        dependencies: &[Dependency],
    ) -> Self {
        let mut descendants: BTreeMap<ContentId, Vec<ContentId>> = BTreeMap::new();
        for edge in dependencies {
            if edge.relationship == Relationship::Derivation {
                descendants
                    .entry(edge.source)
                    .or_default()
                    .push(edge.dependent);
            }
        }
        let mut delete: BTreeSet<_> = roots.into_iter().collect();
        let mut pending: VecDeque<_> = delete.iter().copied().collect();
        while let Some(source) = pending.pop_front() {
            if let Some(children) = descendants.get(&source) {
                for &dependent in children {
                    if delete.insert(dependent) {
                        pending.push_back(dependent);
                    }
                }
            }
        }
        let invalidate_support = dependencies
            .iter()
            .filter(|edge| {
                edge.relationship == Relationship::Citation
                    && delete.contains(&edge.source)
                    && !delete.contains(&edge.dependent)
            })
            .map(|edge| edge.dependent)
            .collect();
        Self {
            delete,
            invalidate_support,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_source_history_is_purged_transitively_but_independent_assertions_survive() {
        let a = ContentId::new();
        let b = ContentId::new();
        let summary = ContentId::new();
        let old_memory = ContentId::new();
        let new_memory = ContentId::new();
        let assertion = ContentId::new();
        let derive = |source, dependent| Dependency {
            source,
            dependent,
            relationship: Relationship::Derivation,
        };
        let edges = [
            derive(a, summary),
            derive(b, summary),
            derive(summary, old_memory),
            derive(old_memory, new_memory),
            derive(new_memory, summary), // corrupt cycle must not hang planning
            Dependency {
                source: a,
                dependent: assertion,
                relationship: Relationship::Citation,
            },
        ];
        let plan = PurgePlan::from_dependencies([a], &edges);
        assert_eq!(
            plan.delete,
            BTreeSet::from([a, summary, old_memory, new_memory])
        );
        assert_eq!(plan.invalidate_support, BTreeSet::from([assertion]));
        assert!(!plan.delete.contains(&b));
    }
}
