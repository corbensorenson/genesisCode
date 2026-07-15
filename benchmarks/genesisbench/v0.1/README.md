# GenesisBench v0.1

Normative profile: `docs/spec/GENESISBENCH_PROTOCOL_v0.1.json`.

`contamination.fixture.json` is the run-bound evidence record for the public local-model conformance run. `eligibility.fixture.json` is its canonical deterministic classification. The run is truthfully in `open-agent`: it uses a disclosed custom fixture scaffold, but its GenesisCode-specific training provenance is `unknown`. It is intentionally unranked and declared-contaminated because the reference is public, the model is a conformance fixture, provenance is incomplete, and no independent registry rescore occurred. Regenerate only by explicitly running the read-only protocol classifier with both records and review any byte change.

The four tracks and content-addressed cohort contract are R1.4.h. The active temporal epoch has 90 independent private lineages, ten per core class, and the public repository contains commitments, exact balance metadata, a hash-only opening audit, and a maintained capability-lease overlay but no private task material. The fixed Cold Acquisition scaffold is R1.4.k and the signed append-only registry is R1.4.m. None is implied by this public conformance fixture beyond its own track/cohort contract.

The public task matrix is nine independent task lineages under three context conditions, not 27 independent challenges. `analysis/observations.fixture.json` is a complete synthetic conformance matrix and `analysis/report.fixture.json` is its deterministic, unranked analysis. Validate both against the predeclared plan with:

```sh
python3 scripts/lib/genesisbench_analysis.py --check --self-test
```

The report counts missing, invalid, abstained, solved, and unsolved cells exactly; computes headline solve intervals over one primary condition per lineage; keeps context ablations clustered by lineage; applies paired exact tests and Holm correction; and refuses rank or saturation claims for public conformance evidence.
