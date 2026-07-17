# GenesisBench Authority Archive

This append-only directory retains exact content-addressed authorities needed to validate historical Open Agent campaigns after generated current authorities advance.

- `suites/<contentIdentitySha256>.json` stores the exact public task suite used by a campaign.
- `protocols/<contentIdentitySha256>.json` stores the exact GenesisBench protocol used by a campaign.
- Filenames must equal the document's recomputed domain-specific content identity.
- Existing files are immutable. A new authority adds a new identity-addressed file; it never replaces an older file.
- Campaign validation resolves the current authority first, then this archive by the identity committed in `campaign.json`.

The archive does not make a campaign rankable. Model, runtime, scaffold, contamination, scoring, hardware, and registry admission requirements remain unchanged.
