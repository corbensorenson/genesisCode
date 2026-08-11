use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{LANGUAGE_PROFILE_ID, Term, TermOrdKey, hash_term, print_term};

use crate::{ModuleForTypecheck, resolve_module_profile};

pub const PROFILE_NEGOTIATION_PROFILE_ID: &str = "genesis/profile-negotiation-v0.1";
pub const PROFILE_FAMILY_LANGUAGE: &str = "genesis/profile-family/language";
pub const PROFILE_FAMILY_CAPABILITY: &str = "genesis/profile-family/capability";
pub const PROFILE_FAMILY_ARTIFACT: &str = "genesis/profile-family/artifact";
pub const PROFILE_FAMILY_TARGET: &str = "genesis/profile-family/target";

pub const PURE_CAPABILITY_PROFILE_ID: &str = "genesis/capability-profile/pure-v0.1";
pub const HOST_ABI_CAPABILITY_PROFILE_ID: &str = "genesis/capability-profile/host-abi-v0.1";
pub const COREFORM_ARTIFACT_PROFILE_ID: &str = "genesis/artifact-profile/coreform-v0.2";
pub const PORTABLE_HOST_TARGET_PROFILE_ID: &str = "genesis/target-profile/portable-host-v0.1";

const REQUIRED_FAMILIES: [&str; 4] = [
    PROFILE_FAMILY_ARTIFACT,
    PROFILE_FAMILY_CAPABILITY,
    PROFILE_FAMILY_LANGUAGE,
    PROFILE_FAMILY_TARGET,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProfileRequirementMode {
    Exact,
    Minimum,
}

impl ProfileRequirementMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "exact" => Some(Self::Exact),
            "minimum" => Some(Self::Minimum),
            _ => None,
        }
    }

    fn as_symbol(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Minimum => "minimum",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProfileRequirement {
    pub mode: ProfileRequirementMode,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOffer {
    profiles: BTreeMap<String, BTreeSet<String>>,
}

impl ProfileOffer {
    pub fn from_profiles(
        profiles: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, String> {
        let mut by_family = BTreeMap::<String, BTreeSet<String>>::new();
        for (family, profile) in profiles {
            let Some(lineage) = profile_lineage(&family) else {
                return Err(format!("unknown profile family {family}"));
            };
            if !lineage.contains(&profile.as_str()) {
                return Err(format!(
                    "profile {profile} is not registered in family {family}"
                ));
            }
            if !by_family.entry(family.clone()).or_default().insert(profile) {
                return Err(format!("duplicate offered profile in family {family}"));
            }
        }
        Ok(Self {
            profiles: by_family,
        })
    }

    pub fn core_host() -> Self {
        let profiles = [
            (PROFILE_FAMILY_LANGUAGE, &[LANGUAGE_PROFILE_ID][..]),
            (
                PROFILE_FAMILY_CAPABILITY,
                &[PURE_CAPABILITY_PROFILE_ID, HOST_ABI_CAPABILITY_PROFILE_ID][..],
            ),
            (PROFILE_FAMILY_ARTIFACT, &[COREFORM_ARTIFACT_PROFILE_ID][..]),
            (
                PROFILE_FAMILY_TARGET,
                &[PORTABLE_HOST_TARGET_PROFILE_ID][..],
            ),
        ]
        .into_iter()
        .map(|(family, members)| {
            (
                family.to_string(),
                members.iter().map(|member| (*member).to_string()).collect(),
            )
        })
        .collect();
        Self { profiles }
    }

    pub fn profiles(&self, family: &str) -> Option<&BTreeSet<String>> {
        self.profiles.get(family)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileNegotiationReport {
    pub active: bool,
    pub ok: bool,
    pub errors_by_module: BTreeMap<String, Vec<String>>,
    pub requirements: BTreeMap<String, ProfileRequirement>,
    pub selected_profiles: BTreeMap<String, String>,
    pub negotiation_identity: Option<[u8; 32]>,
}

impl ProfileNegotiationReport {
    pub fn to_term(&self) -> Term {
        let errors = self
            .errors_by_module
            .iter()
            .map(|(path, messages)| {
                (
                    TermOrdKey(Term::Str(path.clone())),
                    Term::Vector(messages.iter().cloned().map(Term::Str).collect()),
                )
            })
            .collect();
        Term::Map(
            [
                (TermOrdKey(Term::symbol(":active")), Term::Bool(self.active)),
                (TermOrdKey(Term::symbol(":errors")), Term::Map(errors)),
                (
                    TermOrdKey(Term::symbol(":identity")),
                    self.negotiation_identity
                        .map(|identity| Term::Bytes(identity.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::symbol(PROFILE_NEGOTIATION_PROFILE_ID),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(self.ok)),
                (
                    TermOrdKey(Term::symbol(":requirements")),
                    requirements_term(&self.requirements),
                ),
                (
                    TermOrdKey(Term::symbol(":selected")),
                    selected_profiles_term(&self.selected_profiles),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }
}

pub fn negotiate_package_profiles(
    modules: &[ModuleForTypecheck],
    offer: &ProfileOffer,
) -> ProfileNegotiationReport {
    let active = modules.iter().any(has_negotiation_field);
    if !active {
        return ProfileNegotiationReport {
            active: false,
            ok: true,
            errors_by_module: BTreeMap::new(),
            requirements: BTreeMap::new(),
            selected_profiles: BTreeMap::new(),
            negotiation_identity: None,
        };
    }

    let mut errors = BTreeMap::<String, BTreeSet<String>>::new();
    let resolution = resolve_module_profile(modules);
    if !resolution.active || !resolution.ok {
        for module in modules {
            push_error(
                &mut errors,
                &module.path,
                format!("{PROFILE_NEGOTIATION_PROFILE_ID} requires successful module resolution"),
            );
        }
    }

    let mut parsed = Vec::with_capacity(modules.len());
    for module in modules {
        parsed.push(parse_module_requirements(module, &mut errors));
    }

    let distinct_requirements = parsed
        .iter()
        .filter_map(Option::as_ref)
        .collect::<BTreeSet<_>>();
    if distinct_requirements.len() > 1 {
        for module in modules {
            push_error(
                &mut errors,
                &module.path,
                "package modules declare different :package-profile-requirements".to_string(),
            );
        }
    }

    let requirements = parsed
        .iter()
        .find_map(Option::as_ref)
        .cloned()
        .unwrap_or_default();
    let mut selected_profiles = BTreeMap::new();
    if requirements.len() == REQUIRED_FAMILIES.len() {
        for (family, requirement) in &requirements {
            match select_profile(offer, family, requirement) {
                Ok(selected) => {
                    selected_profiles.insert(family.clone(), selected.to_string());
                }
                Err(message) => {
                    for module in modules {
                        push_error(&mut errors, &module.path, message.clone());
                    }
                }
            }
        }
    }

    for module in modules {
        match module_caps(module) {
            Ok(caps)
                if !caps.is_empty()
                    && selected_profiles
                        .get(PROFILE_FAMILY_CAPABILITY)
                        .is_some_and(|profile| profile == PURE_CAPABILITY_PROFILE_ID) =>
            {
                push_error(
                    &mut errors,
                    &module.path,
                    format!(
                        "capability profile {PURE_CAPABILITY_PROFILE_ID} requires :caps [], got [{}]",
                        caps.join(", ")
                    ),
                );
            }
            Ok(_) => {}
            Err(message) => push_error(&mut errors, &module.path, message),
        }
    }

    let errors_by_module = errors
        .into_iter()
        .map(|(path, messages)| (path, messages.into_iter().collect()))
        .collect::<BTreeMap<_, Vec<_>>>();
    let ok = errors_by_module.is_empty()
        && requirements.len() == REQUIRED_FAMILIES.len()
        && selected_profiles.len() == REQUIRED_FAMILIES.len()
        && resolution.resolution_identity.is_some();
    let negotiation_identity = match (ok, resolution.resolution_identity) {
        (true, Some(resolution_identity)) => Some(hash_term(&negotiation_identity_term(
            resolution_identity,
            &requirements,
            &selected_profiles,
        ))),
        _ => None,
    };

    ProfileNegotiationReport {
        active,
        ok,
        errors_by_module,
        requirements,
        selected_profiles,
        negotiation_identity,
    }
}

pub(super) fn merge_profile_errors(
    report: &ProfileNegotiationReport,
    target: &mut BTreeMap<String, Vec<String>>,
) {
    for (path, errors) in &report.errors_by_module {
        target
            .entry(path.clone())
            .or_default()
            .extend(errors.iter().cloned());
    }
}

fn parse_module_requirements(
    module: &ModuleForTypecheck,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> Option<BTreeMap<String, ProfileRequirement>> {
    let path = module.path.as_str();
    let Some(Term::Map(meta)) = module.meta.as_ref() else {
        push_error(
            errors,
            path,
            "profile negotiation requires map-shaped ::meta".to_string(),
        );
        return None;
    };

    match meta.get(&TermOrdKey(Term::symbol(":profile-negotiation"))) {
        Some(Term::Symbol(profile)) if profile == PROFILE_NEGOTIATION_PROFILE_ID => {}
        Some(other) => push_error(
            errors,
            path,
            format!(
                ":profile-negotiation must be exact symbol {PROFILE_NEGOTIATION_PROFILE_ID}, got {}",
                print_term(other)
            ),
        ),
        None => push_error(
            errors,
            path,
            format!(
                "every module in the package closure must declare :profile-negotiation {PROFILE_NEGOTIATION_PROFILE_ID}"
            ),
        ),
    }

    let Some(value) = meta.get(&TermOrdKey(Term::symbol(":package-profile-requirements"))) else {
        push_error(
            errors,
            path,
            "profile negotiation requires :package-profile-requirements".to_string(),
        );
        return None;
    };
    let Term::Map(entries) = value else {
        push_error(
            errors,
            path,
            ":package-profile-requirements must be a map".to_string(),
        );
        return None;
    };

    let mut requirements = BTreeMap::new();
    for (family, requirement) in entries {
        let Term::Symbol(family) = &family.0 else {
            push_error(
                errors,
                path,
                "profile requirement family keys must be symbols".to_string(),
            );
            continue;
        };
        if profile_lineage(family).is_none() {
            push_error(errors, path, format!("unknown profile family {family}"));
            continue;
        }
        if let Some(requirement) = parse_requirement(family, requirement, path, errors) {
            requirements.insert(family.clone(), requirement);
        }
    }

    let actual = requirements
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = REQUIRED_FAMILIES.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        push_error(
            errors,
            path,
            format!(
                ":package-profile-requirements must exactly declare [{}]",
                REQUIRED_FAMILIES.join(", ")
            ),
        );
    }
    Some(requirements)
}

fn parse_requirement(
    family: &str,
    value: &Term,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> Option<ProfileRequirement> {
    let Term::Map(fields) = value else {
        push_error(
            errors,
            path,
            format!("requirement for {family} must be a map"),
        );
        return None;
    };
    let keys = fields.keys().cloned().collect::<BTreeSet<_>>();
    let expected = [
        TermOrdKey(Term::symbol(":mode")),
        TermOrdKey(Term::symbol(":profile")),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if keys != expected {
        push_error(
            errors,
            path,
            format!("requirement for {family} must contain only :mode and :profile"),
        );
        return None;
    }
    let mode_term = fields.get(&TermOrdKey(Term::symbol(":mode")))?;
    let Term::Symbol(mode_symbol) = mode_term else {
        push_error(
            errors,
            path,
            format!("requirement mode for {family} must be exact or minimum"),
        );
        return None;
    };
    let Some(mode) = ProfileRequirementMode::parse(mode_symbol) else {
        push_error(
            errors,
            path,
            format!("unsupported requirement mode {mode_symbol} for {family}"),
        );
        return None;
    };
    let profile_term = fields.get(&TermOrdKey(Term::symbol(":profile")))?;
    let Term::Symbol(profile) = profile_term else {
        push_error(
            errors,
            path,
            format!("required profile for {family} must be a symbol"),
        );
        return None;
    };
    if !profile_lineage(family).is_some_and(|lineage| lineage.contains(&profile.as_str())) {
        push_error(
            errors,
            path,
            format!("profile {profile} is not registered in family {family}"),
        );
        return None;
    }
    Some(ProfileRequirement {
        mode,
        profile: profile.clone(),
    })
}

fn select_profile<'a>(
    offer: &'a ProfileOffer,
    family: &str,
    requirement: &ProfileRequirement,
) -> Result<&'a str, String> {
    let available = offer.profiles(family);
    let selected = match requirement.mode {
        ProfileRequirementMode::Exact => available
            .and_then(|profiles| profiles.get(&requirement.profile))
            .map(String::as_str),
        ProfileRequirementMode::Minimum => {
            let Some(lineage) = profile_lineage(family) else {
                return Err(format!("unknown profile family {family}"));
            };
            let Some(minimum) = lineage
                .iter()
                .position(|profile| *profile == requirement.profile)
            else {
                return Err(format!(
                    "profile {} is not registered in family {family}",
                    requirement.profile
                ));
            };
            lineage.iter().skip(minimum).find_map(|candidate| {
                available
                    .and_then(|profiles| profiles.get(*candidate))
                    .map(String::as_str)
            })
        }
    };
    selected.ok_or_else(|| {
        let offered = available
            .into_iter()
            .flat_map(BTreeSet::iter)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "unsupported {} profile requirement {} in family {family}; offered [{offered}]",
            requirement.mode.as_symbol(),
            requirement.profile,
        )
    })
}

fn negotiation_identity_term(
    resolution_identity: [u8; 32],
    requirements: &BTreeMap<String, ProfileRequirement>,
    selected_profiles: &BTreeMap<String, String>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(PROFILE_NEGOTIATION_PROFILE_ID),
            ),
            (
                TermOrdKey(Term::symbol(":requirements")),
                requirements_term(requirements),
            ),
            (
                TermOrdKey(Term::symbol(":resolution-h")),
                Term::Bytes(resolution_identity.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":selected")),
                selected_profiles_term(selected_profiles),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn requirements_term(requirements: &BTreeMap<String, ProfileRequirement>) -> Term {
    Term::Map(
        requirements
            .iter()
            .map(|(family, requirement)| {
                (
                    TermOrdKey(Term::Symbol(family.clone())),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":mode")),
                                Term::symbol(requirement.mode.as_symbol()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":profile")),
                                Term::Symbol(requirement.profile.clone()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                )
            })
            .collect(),
    )
}

fn selected_profiles_term(selected_profiles: &BTreeMap<String, String>) -> Term {
    Term::Map(
        selected_profiles
            .iter()
            .map(|(family, profile)| {
                (
                    TermOrdKey(Term::Symbol(family.clone())),
                    Term::Symbol(profile.clone()),
                )
            })
            .collect(),
    )
}

fn profile_lineage(family: &str) -> Option<&'static [&'static str]> {
    match family {
        PROFILE_FAMILY_LANGUAGE => Some(&[LANGUAGE_PROFILE_ID]),
        PROFILE_FAMILY_CAPABILITY => {
            Some(&[PURE_CAPABILITY_PROFILE_ID, HOST_ABI_CAPABILITY_PROFILE_ID])
        }
        PROFILE_FAMILY_ARTIFACT => Some(&[COREFORM_ARTIFACT_PROFILE_ID]),
        PROFILE_FAMILY_TARGET => Some(&[PORTABLE_HOST_TARGET_PROFILE_ID]),
        _ => None,
    }
}

fn module_caps(module: &ModuleForTypecheck) -> Result<Vec<String>, String> {
    let Some(Term::Map(meta)) = module.meta.as_ref() else {
        return Err("profile negotiation requires map-shaped ::meta".to_string());
    };
    let Some(value) = meta.get(&TermOrdKey(Term::symbol(":caps"))) else {
        return Err("profile negotiation requires :caps".to_string());
    };
    let Term::Vector(caps) = value else {
        return Err("profile negotiation requires :caps to be a symbol vector".to_string());
    };
    let mut parsed = BTreeSet::new();
    for cap in caps {
        let Term::Symbol(symbol) = cap else {
            return Err(format!(
                "profile negotiation requires :caps entries to be symbols, got {}",
                print_term(cap)
            ));
        };
        if !parsed.insert(symbol.clone()) {
            return Err(format!("duplicate :caps entry {symbol}"));
        }
    }
    Ok(parsed.into_iter().collect())
}

fn has_negotiation_field(module: &ModuleForTypecheck) -> bool {
    matches!(
        module.meta.as_ref(),
        Some(Term::Map(meta)) if meta.contains_key(&TermOrdKey(Term::symbol(":profile-negotiation")))
            || meta.contains_key(&TermOrdKey(Term::symbol(":package-profile-requirements")))
    )
}

fn push_error(errors: &mut BTreeMap<String, BTreeSet<String>>, path: &str, message: String) {
    errors.entry(path.to_string()).or_default().insert(message);
}
