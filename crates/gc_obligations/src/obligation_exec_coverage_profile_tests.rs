use super::*;

#[test]
fn mcdc_independence_detects_single_condition_flip() {
    let conditions: BTreeSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
    let samples = vec![
        DecisionSample {
            conditions: [("a".to_string(), true), ("b".to_string(), true)]
                .into_iter()
                .collect(),
            outcome: true,
        },
        DecisionSample {
            conditions: [("a".to_string(), false), ("b".to_string(), true)]
                .into_iter()
                .collect(),
            outcome: false,
        },
        DecisionSample {
            conditions: [("a".to_string(), true), ("b".to_string(), false)]
                .into_iter()
                .collect(),
            outcome: false,
        },
    ];
    let mcdc = mcdc_independence_for_site(&samples, &conditions);
    assert_eq!(mcdc.get("a"), Some(&true));
    assert_eq!(mcdc.get("b"), Some(&true));
}

#[test]
fn mcdc_independence_fails_when_outcome_never_changes() {
    let conditions: BTreeSet<String> = ["a".to_string()].into_iter().collect();
    let samples = vec![
        DecisionSample {
            conditions: [("a".to_string(), true)].into_iter().collect(),
            outcome: true,
        },
        DecisionSample {
            conditions: [("a".to_string(), false)].into_iter().collect(),
            outcome: true,
        },
    ];
    let mcdc = mcdc_independence_for_site(&samples, &conditions);
    assert_eq!(mcdc.get("a"), Some(&false));
}
