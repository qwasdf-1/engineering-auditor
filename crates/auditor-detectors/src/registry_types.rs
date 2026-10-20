use auditor_core::Severity;

use super::language_support::{language_support, support_policy_for};
use super::{detector_catalog, DetectorOwner};
use crate::{DetectorDefinition, ScanProfile, SourceRole};

const ALL_ROLES: &[SourceRole] = &[
    SourceRole::Production,
    SourceRole::Tooling,
    SourceRole::Test,
    SourceRole::Fixture,
    SourceRole::Benchmark,
    SourceRole::Example,
    SourceRole::Generated,
    SourceRole::Documentation,
    SourceRole::Reference,
];
const PRODUCTION_ROLES: &[SourceRole] = &[SourceRole::Production];
const CODE_ROLES: &[SourceRole] = &[SourceRole::Production, SourceRole::Tooling];
const DOCUMENTATION_ROLES: &[SourceRole] = &[SourceRole::Documentation];
const TEST_SCOPE_ROLES: &[SourceRole] = &[
    SourceRole::Production,
    SourceRole::Tooling,
    SourceRole::Test,
];
const TEST_EVOLUTION_ROLES: &[SourceRole] = &[
    SourceRole::Production,
    SourceRole::Tooling,
    SourceRole::Test,
    SourceRole::Fixture,
    SourceRole::Benchmark,
    SourceRole::Example,
    SourceRole::Reference,
];

const FULL_PROFILE_ONLY: &str =
    "Full profile only: below the precision bar in this language (docs/reference/detector-precision.md).";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RoleApplicability {
    All,
    /// Production code only: style rules whose rationale does not reach
    /// build scripts and maintenance tooling.
    Production,
    Code,
    Documentation,
    #[cfg_attr(not(feature = "graph-analysis"), allow(dead_code))]
    TestEvolution,
    /// Production, tooling, and test code: test functions and the helpers
    /// they delegate to.
    TestScope,
}

impl RoleApplicability {
    pub(crate) const fn roles(self) -> &'static [SourceRole] {
        match self {
            Self::All => ALL_ROLES,
            Self::Production => PRODUCTION_ROLES,
            Self::Code => CODE_ROLES,
            Self::Documentation => DOCUMENTATION_ROLES,
            Self::TestEvolution => TEST_EVOLUTION_ROLES,
            Self::TestScope => TEST_SCOPE_ROLES,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DetectorSpec {
    pub id: &'static str,
    pub category: &'static str,
    pub default_severity: Severity,
    pub minimum_profile: ScanProfile,
    pub layer: &'static str,
    pub owner: DetectorOwner,
    pub role_applicability: RoleApplicability,
    pub context_applicability: RoleApplicability,
    /// Languages where a gating detector is below the precision bar: it runs
    /// there only in the `full` profile.
    pub full_profile_languages: &'static [&'static str],
}

impl DetectorSpec {
    pub(super) const fn full_profile_in(mut self, languages: &'static [&'static str]) -> Self {
        self.full_profile_languages = languages;
        self
    }

    /// Whether a scan at `profile` reports this detector's findings in `language`.
    pub(crate) fn reports_in(&self, profile: ScanProfile, language: &str) -> bool {
        profile.includes(ScanProfile::Full) || !self.full_profile_languages.contains(&language)
    }

    /// Whether this detector emits findings in files of `role`. Index-backed
    /// detectors see every code file, so emission is checked on the finding.
    /// A role that is not a file role (repository-level findings) passes.
    pub(crate) fn emits_role(&self, role: &str) -> bool {
        let roles = self.role_applicability.roles();
        roles.iter().any(|known| known.as_str() == role)
            || !ALL_ROLES.iter().any(|known| known.as_str() == role)
    }

    /// `claim` as a scan at `profile` reports it: a language scoped out of
    /// gating says so.
    pub(crate) fn profile_claim(
        &self,
        profile: ScanProfile,
        claim: crate::DetectorLanguageSupport,
    ) -> crate::DetectorLanguageSupport {
        if self.reports_in(profile, claim.language) {
            return claim;
        }
        crate::DetectorLanguageSupport {
            limitation: Some(FULL_PROFILE_ONLY),
            ..claim
        }
    }

    pub(super) const fn with_roles(mut self, roles: RoleApplicability) -> Self {
        self.role_applicability = roles;
        self.context_applicability = roles;
        self
    }

    pub(super) const fn with_context_roles(mut self, roles: RoleApplicability) -> Self {
        self.context_applicability = roles;
        self
    }
}

pub fn detector_registry(profile: ScanProfile) -> Vec<DetectorDefinition> {
    detector_catalog(profile)
        .map(|spec| DetectorDefinition {
            id: spec.id,
            category: spec.category,
            default_severity: spec.default_severity,
            profile: spec.minimum_profile.as_str(),
            maturity: "v0",
            layer: spec.layer,
            emission_roles: spec.role_applicability.roles(),
            context_roles: spec.context_applicability.roles(),
            language_support: language_support(spec.id, support_policy_for(spec.id))
                .into_iter()
                .map(|claim| spec.profile_claim(profile, claim))
                .collect(),
        })
        .collect()
}
