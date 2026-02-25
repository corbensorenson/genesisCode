#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AssuranceProfile {
    Custom,
    Do178cDalA,
    Do178cDalB,
    NasaClassA,
    NasaClassB,
    Iec62304ClassC,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AssuranceRequirements {
    pub(super) minimum_coverage_rank: u8,
    pub(super) require_independence_attestations: bool,
    pub(super) require_object_equivalence: bool,
    pub(super) require_independent_verifier_runs: bool,
}

impl AssuranceProfile {
    pub(super) fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "custom" => Ok(Self::Custom),
            "do178c-dal-a" => Ok(Self::Do178cDalA),
            "do178c-dal-b" => Ok(Self::Do178cDalB),
            "nasa-class-a" => Ok(Self::NasaClassA),
            "nasa-class-b" => Ok(Self::NasaClassB),
            "iec62304-class-c" => Ok(Self::Iec62304ClassC),
            other => Err(format!(
                "unsupported --assurance-profile `{other}`; expected one of custom|do178c-dal-a|do178c-dal-b|nasa-class-a|nasa-class-b|iec62304-class-c"
            )),
        }
    }

    pub(super) fn as_symbol(self) -> &'static str {
        match self {
            Self::Custom => ":custom",
            Self::Do178cDalA => ":do178c-dal-a",
            Self::Do178cDalB => ":do178c-dal-b",
            Self::NasaClassA => ":nasa-class-a",
            Self::NasaClassB => ":nasa-class-b",
            Self::Iec62304ClassC => ":iec62304-class-c",
        }
    }

    pub(super) fn as_name(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::Do178cDalA => "do178c-dal-a",
            Self::Do178cDalB => "do178c-dal-b",
            Self::NasaClassA => "nasa-class-a",
            Self::NasaClassB => "nasa-class-b",
            Self::Iec62304ClassC => "iec62304-class-c",
        }
    }

    pub(super) fn requirements(self) -> AssuranceRequirements {
        match self {
            Self::Custom => AssuranceRequirements {
                minimum_coverage_rank: 0,
                require_independence_attestations: false,
                require_object_equivalence: false,
                require_independent_verifier_runs: false,
            },
            Self::Do178cDalA => AssuranceRequirements {
                minimum_coverage_rank: 3,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::Do178cDalB => AssuranceRequirements {
                minimum_coverage_rank: 2,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::NasaClassA => AssuranceRequirements {
                minimum_coverage_rank: 3,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::NasaClassB => AssuranceRequirements {
                minimum_coverage_rank: 2,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::Iec62304ClassC => AssuranceRequirements {
                minimum_coverage_rank: 1,
                require_independence_attestations: false,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
        }
    }
}
